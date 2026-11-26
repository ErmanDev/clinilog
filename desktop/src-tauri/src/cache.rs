//! Offline cache (spec §8): a local SQLite file whose sensitive values are all
//! encrypted (see `crypto.rs`). It holds:
//!
//! - `m_sections`, `m_students`, `m_medicines`, `m_visits`, `m_staff`: the school's
//!   clinic records as the server last sent them (the *base*, see `mirror.rs`)
//! - `credentials`: a password check value per account, for offline login. Every
//!   active account of the school comes down with each sync (the server's Argon2
//!   hash), so anyone can log in offline on a PC that has been online once.
//! - `outbox`: changes made on this PC that haven't reached the server yet
//! - `views`: student and visit records opened offline, for the server's audit trail
//! - `responses`: the last copy of pages that need the server (user accounts,
//!   activity log, subscription), per user, shown read-only while offline
//!
//! Logging out keeps all of this, so the PC can still be used offline afterwards.
//! Connecting the PC to another school clears it.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

use crate::{
    api::{SchoolInfo, UserInfo},
    crypto::{self, Cipher},
    local,
    mirror::{Change, Cursors, Mirror, SyncPage, Table},
};

const DB_FILE: &str = "offline-cache.sqlite3";
/// Saved copies of server-only pages kept per user (oldest dropped first).
const MAX_RESPONSES: i64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub user: UserInfo,
    pub school: SchoolInfo,
    /// Argon2id hash of the password: from the server's account list, or made on
    /// this PC at an online login.
    pub password_hash: String,
    pub saved_at: DateTime<Utc>,
}

fn post() -> String {
    "POST".into()
}
fn visits_path() -> String {
    "visits".into()
}
fn new_visit() -> String {
    "New visit".into()
}

/// A change saved on this PC, uploaded later as `method path` with `body`.
///
/// Older versions only queued new visits (`{ body, visited_at, student_label, complaint,
/// recorded_by }`); the defaults and aliases read those as `POST visits`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingOp {
    #[serde(default = "post")]
    pub method: String,
    #[serde(default = "visits_path")]
    pub path: String,
    pub body: Option<serde_json::Value>,
    /// When the change was made.
    #[serde(alias = "visited_at")]
    pub at: DateTime<Utc>,
    /// e.g. "New visit", "Student record edited".
    #[serde(default = "new_visit")]
    pub action: String,
    /// Usually the student, e.g. "Dela Cruz, Juan".
    #[serde(alias = "student_label")]
    pub label: String,
    /// e.g. the complaint.
    #[serde(alias = "complaint")]
    pub summary: String,
    pub recorded_by: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutboxItem {
    pub id: Uuid,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
    /// `pending` (will be retried) or `failed` (the server refused it; needs a person).
    pub status: String,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub op: PendingOp,
}

/// A record opened while offline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewEntry {
    pub table_name: String,
    pub record_id: Uuid,
    pub viewed_at: DateTime<Utc>,
}

struct Inner {
    conn: Connection,
    /// The base, decrypted on first use and kept in memory until logout.
    base: Option<Mirror>,
    /// Base + pending changes; rebuilt after any change to either.
    view: Option<Arc<Mirror>>,
    /// Goes up whenever the copy is cleared (logout, school change), so a download
    /// that started before can't write into the cleared copy.
    epoch: u64,
}

pub struct Cache {
    inner: Mutex<Inner>,
    cipher: Cipher,
}

type R<T> = Result<T, String>;

fn db_err(e: rusqlite::Error) -> String {
    format!("offline cache error: {e}")
}

impl Cache {
    /// Opens (or creates) the cache in `dir`. If this PC's key had to be recreated,
    /// any old cache file is unreadable, so it is replaced with an empty one.
    pub fn open(dir: &Path) -> R<Self> {
        let (key, new_key) = crypto::load_or_create_key(dir).map_err(|e| e.to_string())?;
        let path = dir.join(DB_FILE);
        if new_key && path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        let conn = Connection::open(&path).map_err(db_err)?;
        Self::init(conn, Cipher::new(&key))
    }

    #[cfg(test)]
    pub fn in_memory(key: [u8; 32]) -> Self {
        Self::init(Connection::open_in_memory().unwrap(), Cipher::new(&key)).unwrap()
    }

    fn init(conn: Connection, cipher: Cipher) -> R<Self> {
        conn.execute_batch(
            "PRAGMA secure_delete = ON;
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS credentials (username TEXT PRIMARY KEY, payload BLOB NOT NULL);
             -- Replaced by the m_* tables (the full offline copy).
             DROP TABLE IF EXISTS students;
             DROP TABLE IF EXISTS medicines;
             CREATE TABLE IF NOT EXISTS m_sections (id TEXT PRIMARY KEY, payload BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS m_students (id TEXT PRIMARY KEY, payload BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS m_medicines (id TEXT PRIMARY KEY, payload BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS m_visits (id TEXT PRIMARY KEY, payload BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS m_staff (id TEXT PRIMARY KEY, payload BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS outbox (
                 id TEXT PRIMARY KEY,
                 school_id TEXT NOT NULL,
                 user_id TEXT NOT NULL,
                 created_at TEXT NOT NULL,
                 status TEXT NOT NULL CHECK (status IN ('pending', 'failed')),
                 attempts INTEGER NOT NULL DEFAULT 0,
                 last_error BLOB,
                 payload BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS views (
                 id TEXT PRIMARY KEY,
                 school_id TEXT NOT NULL,
                 user_id TEXT NOT NULL,
                 payload BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS responses (
                 user_id TEXT NOT NULL,
                 key TEXT NOT NULL,
                 saved_at TEXT NOT NULL,
                 payload BLOB NOT NULL,
                 PRIMARY KEY (user_id, key)
             );",
        )
        .map_err(db_err)?;
        Ok(Self { inner: Mutex::new(Inner { conn, base: None, view: None, epoch: 0 }), cipher })
    }

    fn seal<T: Serialize>(&self, aad: &str, value: &T) -> Vec<u8> {
        self.cipher.seal(aad, &serde_json::to_vec(value).expect("serializable"))
    }

    fn unseal<T: DeserializeOwned>(&self, aad: &str, blob: &[u8]) -> R<T> {
        let raw = self.cipher.open(aad, blob).map_err(|e| e.to_string())?;
        serde_json::from_slice(&raw).map_err(|e| format!("offline cache value is unreadable: {e}"))
    }

    // ---------- meta ----------

    fn meta_get_in(conn: &Connection, key: &str) -> R<Option<String>> {
        conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0)).optional().map_err(db_err)
    }

    fn meta_set_in(conn: &Connection, key: &str, value: &str) -> R<()> {
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map_err(db_err)?;
        Ok(())
    }

    fn meta_get(&self, key: &str) -> R<Option<String>> {
        Self::meta_get_in(&self.inner.lock().unwrap().conn, key)
    }

    /// The cache belongs to one school. If this install now points at another school,
    /// the old school's records and logins are dropped.
    pub fn ensure_school(&self, school_id: Uuid) -> R<()> {
        let current = self.meta_get("school_id")?;
        if current.as_deref() != Some(&school_id.to_string()) {
            if current.is_some() {
                self.clear_reference_data(true)?;
            }
            Self::meta_set_in(&self.inner.lock().unwrap().conn, "school_id", &school_id.to_string())?;
        }
        Ok(())
    }

    /// When the offline copy was last brought up to date from the server.
    pub fn synced_at(&self) -> R<Option<DateTime<Utc>>> {
        Ok(self.meta_get("synced_at")?.and_then(|v| v.parse().ok()))
    }

    #[cfg(test)]
    pub fn cursors(&self) -> R<Option<Cursors>> {
        Ok(self.meta_get("sync_cursors")?.and_then(|v| serde_json::from_str(&v).ok()))
    }

    // ---------- credentials ----------

    pub fn save_credentials(&self, username: &str, creds: &Credentials) -> R<()> {
        let key = username.to_lowercase();
        let blob = self.seal(&format!("credentials:{key}"), creds);
        self.inner
            .lock()
            .unwrap()
            .conn
            .execute(
                "INSERT INTO credentials (username, payload) VALUES (?1, ?2)
                 ON CONFLICT(username) DO UPDATE SET payload = excluded.payload",
                params![key, blob],
            )
            .map_err(db_err)?;
        Ok(())
    }

    pub fn credentials(&self, username: &str) -> R<Option<Credentials>> {
        let key = username.to_lowercase();
        let blob: Option<Vec<u8>> = self
            .inner
            .lock()
            .unwrap()
            .conn
            .query_row("SELECT payload FROM credentials WHERE username = ?1", params![key], |r| r.get(0))
            .optional()
            .map_err(db_err)?;
        blob.map(|b| self.unseal(&format!("credentials:{key}"), &b)).transpose()
    }

    // ---------- the offline copy ----------

    fn load_base(&self, conn: &Connection) -> R<Mirror> {
        let mut m = Mirror::default();
        for table in Table::ALL {
            let name = table.sql_name();
            let mut stmt = conn.prepare(&format!("SELECT id, payload FROM {name}")).map_err(db_err)?;
            let rows: Vec<(String, Vec<u8>)> = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(db_err)?
                .collect::<Result<_, _>>()
                .map_err(db_err)?;
            for (id, blob) in rows {
                let value: serde_json::Value = self.unseal(&format!("{name}:{id}"), &blob)?;
                let row = Change::from_payload(table, value).map_err(|e| format!("offline cache row is unreadable: {e}"))?;
                m.apply(&row);
            }
        }
        Ok(m)
    }

    fn base_mut<'a>(&self, inner: &'a mut Inner) -> R<&'a mut Mirror> {
        if inner.base.is_none() {
            inner.base = Some(self.load_base(&inner.conn)?);
        }
        Ok(inner.base.as_mut().expect("loaded"))
    }

    fn persist(&self, tx: &rusqlite::Transaction, changes: &[Change]) -> R<()> {
        for c in changes {
            let name = c.table().sql_name();
            let id = c.id().to_string();
            match c.payload() {
                Some(p) => tx
                    .execute(
                        &format!("INSERT INTO {name} (id, payload) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET payload = excluded.payload"),
                        params![id, self.seal(&format!("{name}:{id}"), &p)],
                    )
                    .map_err(db_err)?,
                None => tx.execute(&format!("DELETE FROM {name} WHERE id = ?1"), params![id]).map_err(db_err)?,
            };
        }
        Ok(())
    }

    /// Where the next download starts (`None` = from scratch), and the copy's epoch,
    /// which the download hands back to [`Self::apply_sync_page`].
    pub fn sync_start(&self) -> R<(Option<Cursors>, u64)> {
        let inner = self.inner.lock().unwrap();
        let cursors = Self::meta_get_in(&inner.conn, "sync_cursors")?.and_then(|v| serde_json::from_str(&v).ok());
        Ok((cursors, inner.epoch))
    }

    /// Applies one page of `GET /sync` to the base and remembers where it left off.
    /// `fresh` = the first page of a sync from scratch: everything held before is dropped.
    /// Refused if the copy was cleared since the download started (`epoch` changed).
    pub fn apply_sync_page(&self, page: SyncPage, fresh: bool, epoch: u64) -> R<()> {
        let mut inner = self.inner.lock().unwrap();
        if inner.epoch != epoch {
            return Err("The offline copy was cleared while it was downloading.".into());
        }
        if fresh {
            for t in Table::ALL {
                inner.conn.execute(&format!("DELETE FROM {}", t.sql_name()), []).map_err(db_err)?;
            }
            inner.base = Some(Mirror::default());
        }
        let next = page.next.clone();
        let accounts = match (page.school.clone(), page.accounts.clone()) {
            (Some(school), Some(list)) => Some((school, list)),
            _ => None,
        };
        let changes = page.changes(self.base_mut(&mut inner)?);
        let tx = inner.conn.transaction().map_err(db_err)?;
        self.persist(&tx, &changes)?;
        if let Some((school, list)) = accounts {
            // The full list: an account missing from it was disabled or deleted.
            tx.execute("DELETE FROM credentials", []).map_err(db_err)?;
            for a in list {
                let key = a.username.to_lowercase();
                let creds = Credentials {
                    user: UserInfo { id: a.id, full_name: a.full_name, username: a.username, role: a.role },
                    school: school.clone(),
                    password_hash: a.password_hash,
                    saved_at: Utc::now(),
                };
                tx.execute(
                    "INSERT INTO credentials (username, payload) VALUES (?1, ?2)
                     ON CONFLICT(username) DO UPDATE SET payload = excluded.payload",
                    params![key, self.seal(&format!("credentials:{key}"), &creds)],
                )
                .map_err(db_err)?;
            }
        }
        Self::meta_set_in(&tx, "sync_cursors", &serde_json::to_string(&next).expect("serializable"))?;
        Self::meta_set_in(&tx, "synced_at", &Utc::now().to_rfc3339())?;
        tx.commit().map_err(db_err)?;
        let base = self.base_mut(&mut inner)?;
        base.apply_all(&changes);
        if !changes.is_empty() || fresh {
            inner.view = None;
        }
        Ok(())
    }

    /// Puts rows straight into the base: a change the server just accepted, so it
    /// shows until the next sync brings the server's own copy.
    pub fn apply_to_base(&self, changes: &[Change]) -> R<()> {
        if changes.is_empty() {
            return Ok(());
        }
        let mut inner = self.inner.lock().unwrap();
        let tx = inner.conn.transaction().map_err(db_err)?;
        self.persist(&tx, changes)?;
        tx.commit().map_err(db_err)?;
        self.base_mut(&mut inner)?.apply_all(changes);
        inner.view = None;
        Ok(())
    }

    /// A waiting change the server accepted: it moves from the outbox into the base,
    /// so it keeps showing until the next sync brings the server's own copy.
    pub fn accept(&self, item: &OutboxItem) -> R<()> {
        let mut inner = self.inner.lock().unwrap();
        let changes = replayed(self.base_mut(&mut inner)?, item.user_id, &item.op.recorded_by, item.op.at, &item.op);
        let tx = inner.conn.transaction().map_err(db_err)?;
        self.persist(&tx, &changes)?;
        tx.execute("DELETE FROM outbox WHERE id = ?1", params![item.id.to_string()]).map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        self.base_mut(&mut inner)?.apply_all(&changes);
        inner.view = None;
        Ok(())
    }

    /// A change that went straight to the server: copied into the base the same way.
    pub fn accept_direct(&self, user_id: Uuid, user_name: &str, method: &str, path: &str, body: Option<&serde_json::Value>) -> R<()> {
        let op = PendingOp {
            method: method.into(),
            path: path.into(),
            body: body.cloned(),
            at: Utc::now(),
            action: String::new(),
            label: String::new(),
            summary: String::new(),
            recorded_by: user_name.into(),
        };
        let changes = {
            let mut inner = self.inner.lock().unwrap();
            replayed(self.base_mut(&mut inner)?, user_id, user_name, op.at, &op)
        };
        self.apply_to_base(&changes)
    }

    /// The records as the app should show them offline: the base plus this school's
    /// changes still waiting to upload (from any user on this PC), in the order made.
    pub fn view(&self) -> R<Arc<Mirror>> {
        // One lock for the whole build, so a change saved meanwhile can't be left out.
        let mut inner = self.inner.lock().unwrap();
        if let Some(v) = &inner.view {
            return Ok(v.clone());
        }
        let school: Option<Uuid> = Self::meta_get_in(&inner.conn, "school_id")?.and_then(|v| v.parse().ok());
        let pending: Vec<OutboxItem> = match school {
            Some(s) => self.outbox_in(&inner.conn, s)?.into_iter().filter(|i| i.status == "pending").collect(),
            None => vec![],
        };
        let mut view = self.base_mut(&mut inner)?.clone();
        for item in &pending {
            replay(&mut view, item);
        }
        let view = Arc::new(view);
        inner.view = Some(view.clone());
        Ok(view)
    }

    // ---------- outbox ----------

    pub fn outbox_add(&self, school_id: Uuid, user_id: Uuid, id: Uuid, op: &PendingOp) -> R<()> {
        let blob = self.seal(&format!("outbox:{id}"), op);
        let mut inner = self.inner.lock().unwrap();
        inner
            .conn
            .execute(
                "INSERT INTO outbox (id, school_id, user_id, created_at, status, payload) VALUES (?1, ?2, ?3, ?4, 'pending', ?5)",
                // Microsecond time + insertion keeps the upload order the same as the order made.
                params![
                    id.to_string(),
                    school_id.to_string(),
                    user_id.to_string(),
                    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
                    blob
                ],
            )
            .map_err(db_err)?;
        inner.view = None;
        Ok(())
    }

    /// Everything in the outbox for this school, oldest first.
    pub fn outbox(&self, school_id: Uuid) -> R<Vec<OutboxItem>> {
        let inner = self.inner.lock().unwrap();
        self.outbox_in(&inner.conn, school_id)
    }

    fn outbox_in(&self, conn: &Connection, school_id: Uuid) -> R<Vec<OutboxItem>> {
        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, created_at, status, attempts, last_error, payload
                   FROM outbox WHERE school_id = ?1 ORDER BY created_at, rowid",
            )
            .map_err(db_err)?;
        type Row = (String, String, String, String, i64, Option<Vec<u8>>, Vec<u8>);
        let rows: Vec<Row> = stmt
            .query_map(params![school_id.to_string()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
            })
            .map_err(db_err)?
            .collect::<Result<_, _>>()
            .map_err(db_err)?;
        rows.into_iter()
            .map(|(id, user_id, created_at, status, attempts, last_error, payload)| {
                let aad = format!("outbox:{id}");
                Ok(OutboxItem {
                    id: id.parse().map_err(|_| "bad outbox id".to_string())?,
                    user_id: user_id.parse().map_err(|_| "bad outbox user".to_string())?,
                    created_at: created_at.parse().map_err(|_| "bad outbox time".to_string())?,
                    status,
                    attempts,
                    last_error: last_error.map(|b| self.unseal::<String>(&format!("{aad}:error"), &b)).transpose()?,
                    op: self.unseal(&aad, &payload)?,
                })
            })
            .collect()
    }

    pub fn outbox_delete(&self, id: Uuid) -> R<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.conn.execute("DELETE FROM outbox WHERE id = ?1", params![id.to_string()]).map_err(db_err)?;
        inner.view = None;
        Ok(())
    }

    /// Records an upload attempt. `failed` = the server refused it (won't be retried automatically).
    pub fn outbox_note(&self, id: Uuid, failed: bool, error: Option<&str>) -> R<()> {
        let err_blob = error.map(|e| self.seal(&format!("outbox:{id}:error"), &e.to_string()));
        let mut inner = self.inner.lock().unwrap();
        inner
            .conn
            .execute(
                "UPDATE outbox SET attempts = attempts + 1, status = ?2, last_error = ?3 WHERE id = ?1",
                params![id.to_string(), if failed { "failed" } else { "pending" }, err_blob],
            )
            .map_err(db_err)?;
        if failed {
            inner.view = None;
        }
        Ok(())
    }

    /// Puts a failed item back in the queue, optionally with a changed body
    /// (e.g. "save anyway" after a possible-duplicate warning).
    pub fn outbox_retry(&self, id: Uuid, op: Option<&PendingOp>) -> R<()> {
        let mut inner = self.inner.lock().unwrap();
        match op {
            Some(op) => inner.conn.execute(
                "UPDATE outbox SET status = 'pending', last_error = NULL, payload = ?2 WHERE id = ?1",
                params![id.to_string(), self.seal(&format!("outbox:{id}"), op)],
            ),
            None => inner.conn.execute("UPDATE outbox SET status = 'pending', last_error = NULL WHERE id = ?1", params![id.to_string()]),
        }
        .map_err(db_err)?;
        inner.view = None;
        Ok(())
    }

    // ---------- records opened offline ----------

    pub fn log_view(&self, school_id: Uuid, user_id: Uuid, view: &ViewEntry) -> R<()> {
        let id = Uuid::new_v4();
        self.inner
            .lock()
            .unwrap()
            .conn
            .execute(
                "INSERT INTO views (id, school_id, user_id, payload) VALUES (?1, ?2, ?3, ?4)",
                params![id.to_string(), school_id.to_string(), user_id.to_string(), self.seal(&format!("views:{id}"), view)],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Up to `limit` views by this user, each with its id (the upload's retry key).
    pub fn views(&self, school_id: Uuid, user_id: Uuid, limit: i64) -> R<Vec<(Uuid, ViewEntry)>> {
        let inner = self.inner.lock().unwrap();
        let mut stmt = inner
            .conn
            .prepare("SELECT id, payload FROM views WHERE school_id = ?1 AND user_id = ?2 ORDER BY rowid LIMIT ?3")
            .map_err(db_err)?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map(params![school_id.to_string(), user_id.to_string(), limit], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(db_err)?
            .collect::<Result<_, _>>()
            .map_err(db_err)?;
        drop(stmt);
        drop(inner);
        rows.into_iter()
            .map(|(id, blob)| Ok((id.parse().map_err(|_| "bad view id".to_string())?, self.unseal(&format!("views:{id}"), &blob)?)))
            .collect()
    }

    pub fn views_delete(&self, ids: &[Uuid]) -> R<()> {
        let mut inner = self.inner.lock().unwrap();
        let tx = inner.conn.transaction().map_err(db_err)?;
        for id in ids {
            tx.execute("DELETE FROM views WHERE id = ?1", params![id.to_string()]).map_err(db_err)?;
        }
        tx.commit().map_err(db_err)
    }

    // ---------- saved copies of server-only pages ----------

    pub fn save_response(&self, user_id: Uuid, key: &str, value: &serde_json::Value) -> R<()> {
        let user = user_id.to_string();
        let blob = self.seal(&format!("responses:{user}:{key}"), value);
        let inner = self.inner.lock().unwrap();
        inner
            .conn
            .execute(
                "INSERT INTO responses (user_id, key, saved_at, payload) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(user_id, key) DO UPDATE SET saved_at = excluded.saved_at, payload = excluded.payload",
                params![user, key, Utc::now().to_rfc3339(), blob],
            )
            .map_err(db_err)?;
        inner
            .conn
            .execute(
                "DELETE FROM responses WHERE user_id = ?1 AND key NOT IN
                   (SELECT key FROM responses WHERE user_id = ?1 ORDER BY saved_at DESC LIMIT ?2)",
                params![user, MAX_RESPONSES],
            )
            .map_err(db_err)?;
        Ok(())
    }

    pub fn response(&self, user_id: Uuid, key: &str) -> R<Option<serde_json::Value>> {
        let user = user_id.to_string();
        let blob: Option<Vec<u8>> = self
            .inner
            .lock()
            .unwrap()
            .conn
            .query_row("SELECT payload FROM responses WHERE user_id = ?1 AND key = ?2", params![user, key], |r| r.get(0))
            .optional()
            .map_err(db_err)?;
        blob.map(|b| self.unseal(&format!("responses:{user}:{key}"), &b)).transpose()
    }

    // ---------- clearing ----------

    /// Drops records, logins and saved pages (when the PC changes school). `school_changed`
    /// also drops views logged for the old school, which can no longer be uploaded.
    fn clear_reference_data(&self, school_changed: bool) -> R<()> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .conn
            .execute_batch(
                "DELETE FROM credentials; DELETE FROM responses;
                 DELETE FROM m_sections; DELETE FROM m_students; DELETE FROM m_medicines;
                 DELETE FROM m_visits; DELETE FROM m_staff;
                 DELETE FROM meta WHERE key IN ('synced_at', 'sync_cursors');",
            )
            .map_err(db_err)?;
        if school_changed {
            inner.conn.execute("DELETE FROM views", []).map_err(db_err)?;
        }
        inner.base = None;
        inner.view = None;
        inner.epoch += 1;
        Ok(())
    }

    /// Drops the decrypted copy held in memory (on logout). The file keeps everything.
    pub fn forget_in_memory(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.base = None;
        inner.view = None;
    }
}

/// The rows a saved change touches when re-applied on top of `m` (best effort: a
/// change that no longer fits, e.g. its student was deleted on the server, gives none).
fn replayed(m: &Mirror, user_id: Uuid, user_name: &str, at: DateTime<Utc>, op: &PendingOp) -> Vec<Change> {
    let ctx = local::Ctx { user_id, user_name, is_admin: true, plan: None, now: at, replay: true };
    match local::write(m, &ctx, &op.method, &op.path, op.body.as_ref()) {
        Some(Ok(w)) => w.changes,
        _ => vec![],
    }
}

fn replay(view: &mut Mirror, item: &OutboxItem) {
    let changes = replayed(view, item.user_id, &item.op.recorded_by, item.op.at, &item.op);
    view.apply_all(&changes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mirror::{MSection, MStudent};
    use chrono::NaiveDate;

    fn creds() -> Credentials {
        Credentials {
            user: UserInfo { id: Uuid::new_v4(), full_name: "Nurse Joy".into(), username: "nurse_joy".into(), role: "staff".into() },
            school: SchoolInfo { id: Uuid::new_v4(), name: "VNHS".into() },
            password_hash: "$argon2id$x".into(),
            saved_at: Utc::now(),
        }
    }

    fn section() -> MSection {
        MSection { id: Uuid::new_v4(), grade_level: 7, section_name: "Rosal".into() }
    }

    fn student(section_id: Uuid) -> MStudent {
        MStudent {
            id: Uuid::new_v4(),
            lrn: "123456789012".into(),
            first_name: "Juan".into(),
            middle_name: None,
            last_name: "Dela Cruz".into(),
            gender: "male".into(),
            birthdate: NaiveDate::from_ymd_opt(2012, 5, 1).unwrap(),
            section_id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn page(sections: Vec<MSection>, students: Vec<MStudent>) -> SyncPage {
        serde_json::from_value(serde_json::json!({
            "sections": sections, "medicines": [], "staff": [], "students": students, "visits": [], "deleted": [],
            "next": { "students": "2026-11-26T00:00:00Z|00000000-0000-0000-0000-000000000000", "visits": null, "deleted": null },
            "more": false
        }))
        .unwrap()
    }

    fn new_visit_op(student: Uuid) -> PendingOp {
        PendingOp {
            method: "POST".into(),
            path: "visits".into(),
            body: Some(serde_json::json!({ "id": Uuid::new_v4(), "student_id": student, "complaint": "Headache", "treatment": "Rest", "medicines": [] })),
            at: Utc::now(),
            action: "New visit".into(),
            label: "Dela Cruz, Juan".into(),
            summary: "Headache".into(),
            recorded_by: "Nurse Joy".into(),
        }
    }

    #[test]
    fn view_is_the_base_plus_pending_changes() {
        let c = Cache::in_memory([1; 32]);
        let school = Uuid::new_v4();
        c.ensure_school(school).unwrap();
        let sec = section();
        let s = student(sec.id);
        c.apply_sync_page(page(vec![sec], vec![s.clone()]), true, c.sync_start().unwrap().1).unwrap();
        assert!(c.synced_at().unwrap().is_some() && c.cursors().unwrap().unwrap().students.is_some());

        let (user, op_id) = (Uuid::new_v4(), Uuid::new_v4());
        c.outbox_add(school, user, op_id, &new_visit_op(s.id)).unwrap();
        assert_eq!(c.view().unwrap().visits.len(), 1, "pending visit shows offline");

        // Refused by the server: it drops out of the view but stays listed.
        c.outbox_note(op_id, true, Some("Choose a student from the list.")).unwrap();
        assert!(c.view().unwrap().visits.is_empty());
        c.outbox_retry(op_id, None).unwrap();
        assert_eq!(c.view().unwrap().visits.len(), 1);

        // A later sync doesn't hide changes that are still waiting.
        c.apply_sync_page(page(vec![], vec![]), false, c.sync_start().unwrap().1).unwrap();
        assert_eq!(c.view().unwrap().visits.len(), 1);
        assert!(c.view().unwrap().sections.is_empty(), "sections come in full; missing = deleted");
    }

    #[test]
    fn base_survives_reopening_and_rows_are_encrypted() {
        let dir = std::env::temp_dir().join(format!("clinilog-cache-test-{}", Uuid::new_v4()));
        let sec = section();
        {
            let c = Cache::open(&dir).unwrap();
            c.apply_sync_page(page(vec![sec.clone()], vec![student(sec.id)]), true, c.sync_start().unwrap().1).unwrap();
            c.save_credentials("nurse_joy", &creds()).unwrap();
            let user = Uuid::new_v4();
            c.save_response(user, "users", &serde_json::json!([{ "full_name": "Nurse Joy" }])).unwrap();
            c.log_view(Uuid::new_v4(), user, &ViewEntry { table_name: "students".into(), record_id: Uuid::new_v4(), viewed_at: Utc::now() })
                .unwrap();
            let inner = c.inner.lock().unwrap();
            for table in ["m_students", "credentials", "responses", "views"] {
                let blob: Vec<u8> = inner.conn.query_row(&format!("SELECT payload FROM {table}"), [], |r| r.get(0)).unwrap();
                for secret in [&b"Juan"[..], b"123456789012", b"argon2", b"Nurse"] {
                    assert!(!blob.windows(secret.len()).any(|w| w == secret), "{table} leaks plaintext");
                }
            }
        }
        let c = Cache::open(&dir).unwrap();
        assert_eq!(c.view().unwrap().students.len(), 1);
        // Key lost/replaced: the old file is reset, not half-read.
        drop(c);
        std::fs::write(dir.join("offline-cache.key"), b"not a valid protected key").unwrap();
        assert!(Cache::open(&dir).unwrap().view().unwrap().students.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn outbox_lifecycle_and_old_queued_visits_still_read() {
        let c = Cache::in_memory([3; 32]);
        let (school, user) = (Uuid::new_v4(), Uuid::new_v4());
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        c.outbox_add(school, user, a, &new_visit_op(Uuid::new_v4())).unwrap();
        // A visit queued by the previous version of the app.
        let old = serde_json::json!({
            "body": { "id": b, "complaint": "Fever" }, "visited_at": Utc::now(),
            "student_label": "Reyes, Ana", "complaint": "Fever", "recorded_by": "Nurse Joy"
        });
        {
            let inner = c.inner.lock().unwrap();
            inner
                .conn
                .execute(
                    "INSERT INTO outbox (id, school_id, user_id, created_at, status, payload) VALUES (?1, ?2, ?3, ?4, 'pending', ?5)",
                    params![b.to_string(), school.to_string(), user.to_string(), Utc::now().to_rfc3339(), c.seal(&format!("outbox:{b}"), &old)],
                )
                .unwrap();
        }
        let items = c.outbox(school).unwrap();
        assert_eq!(items.len(), 2);
        let ib = items.iter().find(|i| i.id == b).unwrap();
        assert_eq!((ib.op.method.as_str(), ib.op.path.as_str(), ib.op.label.as_str()), ("POST", "visits", "Reyes, Ana"));
        assert!(c.outbox(Uuid::new_v4()).unwrap().is_empty(), "scoped to school");
        c.outbox_note(a, true, Some("Choose a student from the list.")).unwrap();
        assert_eq!(c.outbox(school).unwrap()[0].last_error.as_deref(), Some("Choose a student from the list."));
        c.outbox_delete(a).unwrap();
        assert_eq!(c.outbox(school).unwrap().len(), 1);
    }

    #[test]
    fn logout_keeps_the_offline_copy_and_logins() {
        let c = Cache::in_memory([4; 32]);
        let (school, user) = (Uuid::new_v4(), Uuid::new_v4());
        c.ensure_school(school).unwrap();
        let sec = section();
        c.apply_sync_page(page(vec![sec.clone()], vec![student(sec.id)]), true, c.sync_start().unwrap().1).unwrap();
        c.save_credentials("Nurse_Joy", &creds()).unwrap();
        assert!(c.credentials("NURSE_JOY").unwrap().is_some(), "usernames are case-insensitive");
        c.save_response(user, "users", &serde_json::json!([])).unwrap();
        c.forget_in_memory();
        assert_eq!(c.view().unwrap().students.len(), 1);
        assert!(c.credentials("nurse_joy").unwrap().is_some());
        assert!(c.synced_at().unwrap().is_some() && c.cursors().unwrap().is_some());
        assert!(c.response(user, "users").unwrap().is_some());
    }

    #[test]
    fn synced_accounts_replace_the_saved_logins() {
        let c = Cache::in_memory([7; 32]);
        c.save_credentials("old_nurse", &creds()).unwrap();
        let school = Uuid::new_v4();
        let mut p = page(vec![], vec![]);
        p.school = Some(SchoolInfo { id: school, name: "Valenzuela NHS".into() });
        p.accounts = Some(vec![crate::mirror::SyncAccount {
            id: Uuid::new_v4(),
            full_name: "Clinic Admin".into(),
            username: "Admin".into(),
            role: "admin".into(),
            password_hash: "$argon2id$v=19$server".into(),
        }]);
        c.apply_sync_page(p, true, c.sync_start().unwrap().1).unwrap();
        let admin = c.credentials("admin").unwrap().expect("account from the server");
        assert_eq!((admin.user.role.as_str(), admin.school.id, admin.password_hash.as_str()), ("admin", school, "$argon2id$v=19$server"));
        assert!(c.credentials("old_nurse").unwrap().is_none(), "not on the server's list any more");
        // A server without account sync leaves the saved logins alone.
        c.apply_sync_page(page(vec![], vec![]), false, c.sync_start().unwrap().1).unwrap();
        assert!(c.credentials("admin").unwrap().is_some());
    }

    #[test]
    fn switching_school_drops_the_old_schools_data() {
        let c = Cache::in_memory([5; 32]);
        c.ensure_school(Uuid::new_v4()).unwrap();
        let sec = section();
        c.apply_sync_page(page(vec![sec.clone()], vec![student(sec.id)]), true, c.sync_start().unwrap().1).unwrap();
        c.save_credentials("nurse_joy", &creds()).unwrap();
        c.ensure_school(Uuid::new_v4()).unwrap();
        assert!(c.view().unwrap().students.is_empty());
        assert!(c.credentials("nurse_joy").unwrap().is_none());
    }

    #[test]
    fn a_download_running_during_a_school_change_cant_refill_the_copy() {
        let c = Cache::in_memory([6; 32]);
        c.ensure_school(Uuid::new_v4()).unwrap();
        let (_, epoch) = c.sync_start().unwrap();
        c.ensure_school(Uuid::new_v4()).unwrap();
        let sec = section();
        assert!(c.apply_sync_page(page(vec![sec.clone()], vec![student(sec.id)]), true, epoch).is_err());
        assert!(c.view().unwrap().students.is_empty());
        assert!(c.cursors().unwrap().is_none(), "next login starts from scratch");
    }
}
