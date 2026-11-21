//! Offline cache (spec §8): a local SQLite file whose sensitive values are all
//! encrypted (see `crypto.rs`). It holds only what's needed to log visits offline:
//!
//! - `students`, `medicines`: lists for picking on the visit form
//! - `credentials`: a password check value per user, for offline login
//! - `outbox`: visits saved on this PC that haven't reached the server yet
//!
//! Logging out clears everything except the outbox (unsent visits are never
//! thrown away; they upload the next time their author logs in online).

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{path::Path, sync::Mutex};
use uuid::Uuid;

use crate::{
    api::{SchoolInfo, UserInfo},
    crypto::{self, Cipher},
};

const DB_FILE: &str = "offline-cache.sqlite3";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedStudent {
    pub id: Uuid,
    pub lrn: String,
    pub first_name: String,
    pub middle_name: Option<String>,
    pub last_name: String,
    pub grade_level: i32,
    pub section_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedMedicine {
    pub id: Uuid,
    pub name: String,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub user: UserInfo,
    pub school: SchoolInfo,
    /// Argon2id hash of the password, made on this PC at the last online login.
    pub password_hash: String,
    pub saved_at: DateTime<Utc>,
}

/// A visit saved on this PC. `body` is exactly what will be POSTed to `/visits`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingVisit {
    pub body: serde_json::Value,
    pub visited_at: DateTime<Utc>,
    pub student_label: String,
    pub complaint: String,
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
    pub visit: PendingVisit,
}

pub struct Cache {
    conn: Mutex<Connection>,
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
             CREATE TABLE IF NOT EXISTS students (id TEXT PRIMARY KEY, payload BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS medicines (id TEXT PRIMARY KEY, payload BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS outbox (
                 id TEXT PRIMARY KEY,
                 school_id TEXT NOT NULL,
                 user_id TEXT NOT NULL,
                 created_at TEXT NOT NULL,
                 status TEXT NOT NULL CHECK (status IN ('pending', 'failed')),
                 attempts INTEGER NOT NULL DEFAULT 0,
                 last_error BLOB,
                 payload BLOB NOT NULL
             );",
        )
        .map_err(db_err)?;
        Ok(Self { conn: Mutex::new(conn), cipher })
    }

    fn seal<T: Serialize>(&self, aad: &str, value: &T) -> Vec<u8> {
        self.cipher.seal(aad, &serde_json::to_vec(value).expect("serializable"))
    }

    fn unseal<T: DeserializeOwned>(&self, aad: &str, blob: &[u8]) -> R<T> {
        let raw = self.cipher.open(aad, blob).map_err(|e| e.to_string())?;
        serde_json::from_slice(&raw).map_err(|e| format!("offline cache value is unreadable: {e}"))
    }

    // ---------- meta ----------

    fn meta_get(&self, key: &str) -> R<Option<String>> {
        self.conn
            .lock()
            .unwrap()
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0))
            .optional()
            .map_err(db_err)
    }

    fn meta_set(&self, key: &str, value: &str) -> R<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])
            .map_err(db_err)?;
        Ok(())
    }

    /// The cache belongs to one school. If this install now points at another school,
    /// the old school's lists and logins are dropped.
    pub fn ensure_school(&self, school_id: Uuid) -> R<()> {
        let current = self.meta_get("school_id")?;
        if current.as_deref() != Some(&school_id.to_string()) {
            if current.is_some() {
                self.clear_reference_data()?;
            }
            self.meta_set("school_id", &school_id.to_string())?;
        }
        Ok(())
    }

    pub fn lists_refreshed_at(&self) -> R<Option<DateTime<Utc>>> {
        Ok(self.meta_get("lists_refreshed_at")?.and_then(|v| v.parse().ok()))
    }

    // ---------- credentials ----------

    pub fn save_credentials(&self, username: &str, creds: &Credentials) -> R<()> {
        let key = username.to_lowercase();
        let blob = self.seal(&format!("credentials:{key}"), creds);
        self.conn
            .lock()
            .unwrap()
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
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT payload FROM credentials WHERE username = ?1", params![key], |r| r.get(0))
            .optional()
            .map_err(db_err)?;
        blob.map(|b| self.unseal(&format!("credentials:{key}"), &b)).transpose()
    }

    // ---------- students & medicines ----------

    pub fn replace_lists(&self, students: &[CachedStudent], medicines: &[CachedMedicine]) -> R<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(db_err)?;
        tx.execute("DELETE FROM students", []).map_err(db_err)?;
        tx.execute("DELETE FROM medicines", []).map_err(db_err)?;
        for s in students {
            let id = s.id.to_string();
            tx.execute("INSERT INTO students (id, payload) VALUES (?1, ?2)", params![id, self.seal(&format!("students:{id}"), s)])
                .map_err(db_err)?;
        }
        for m in medicines {
            let id = m.id.to_string();
            tx.execute("INSERT INTO medicines (id, payload) VALUES (?1, ?2)", params![id, self.seal(&format!("medicines:{id}"), m)])
                .map_err(db_err)?;
        }
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('lists_refreshed_at', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![Utc::now().to_rfc3339()],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)
    }

    fn all<T: DeserializeOwned>(&self, table: &'static str) -> R<Vec<T>> {
        let conn = self.conn.lock().unwrap();
        let sql = match table {
            "students" => "SELECT id, payload FROM students",
            "medicines" => "SELECT id, payload FROM medicines",
            _ => unreachable!("fixed table names only"),
        };
        let mut stmt = conn.prepare(sql).map_err(db_err)?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(db_err)?
            .collect::<Result<_, _>>()
            .map_err(db_err)?;
        rows.iter().map(|(id, blob)| self.unseal(&format!("{table}:{id}"), blob)).collect()
    }

    pub fn medicines(&self) -> R<Vec<CachedMedicine>> {
        let mut m: Vec<CachedMedicine> = self.all("medicines")?;
        m.sort_by_key(|x| x.name.to_lowercase());
        Ok(m)
    }

    pub fn student(&self, id: Uuid) -> R<Option<CachedStudent>> {
        Ok(self.all::<CachedStudent>("students")?.into_iter().find(|s| s.id == id))
    }

    /// Same matching as the server: name words in order ("first last" or "last, first"),
    /// or the start of an LRN. Values are decrypted in memory only.
    pub fn search_students(&self, q: &str, limit: usize) -> R<Vec<CachedStudent>> {
        let words: Vec<String> = q.to_lowercase().replace(',', " ").split_whitespace().map(str::to_string).collect();
        if words.is_empty() {
            return Ok(vec![]);
        }
        let in_order = |hay: &str| {
            let mut rest = hay;
            words.iter().all(|w| match rest.find(w.as_str()) {
                Some(i) => {
                    rest = &rest[i + w.len()..];
                    true
                }
                None => false,
            })
        };
        let mut out: Vec<CachedStudent> = self
            .all::<CachedStudent>("students")?
            .into_iter()
            .filter(|s| {
                let mid = s.middle_name.as_deref().unwrap_or("");
                let a = format!("{} {} {}", s.first_name, mid, s.last_name).to_lowercase();
                let b = format!("{} {} {}", s.last_name, s.first_name, mid).to_lowercase();
                (words.len() == 1 && s.lrn.starts_with(&words[0])) || in_order(&a) || in_order(&b)
            })
            .collect();
        out.sort_by(|x, y| {
            (x.last_name.to_lowercase(), x.first_name.to_lowercase()).cmp(&(y.last_name.to_lowercase(), y.first_name.to_lowercase()))
        });
        out.truncate(limit);
        Ok(out)
    }

    // ---------- outbox ----------

    pub fn outbox_add(&self, school_id: Uuid, user_id: Uuid, id: Uuid, visit: &PendingVisit) -> R<()> {
        let blob = self.seal(&format!("outbox:{id}"), visit);
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO outbox (id, school_id, user_id, created_at, status, payload) VALUES (?1, ?2, ?3, ?4, 'pending', ?5)",
                params![id.to_string(), school_id.to_string(), user_id.to_string(), Utc::now().to_rfc3339(), blob],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Everything in the outbox for this school, oldest first.
    pub fn outbox(&self, school_id: Uuid) -> R<Vec<OutboxItem>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, created_at, status, attempts, last_error, payload
                   FROM outbox WHERE school_id = ?1 ORDER BY created_at, id",
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
        drop(stmt);
        drop(conn);
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
                    visit: self.unseal(&aad, &payload)?,
                })
            })
            .collect()
    }

    pub fn outbox_delete(&self, id: Uuid) -> R<()> {
        self.conn.lock().unwrap().execute("DELETE FROM outbox WHERE id = ?1", params![id.to_string()]).map_err(db_err)?;
        Ok(())
    }

    /// Records an upload attempt. `failed` = the server refused it (won't be retried automatically).
    pub fn outbox_note(&self, id: Uuid, failed: bool, error: Option<&str>) -> R<()> {
        let err_blob = error.map(|e| self.seal(&format!("outbox:{id}:error"), &e.to_string()));
        self.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE outbox SET attempts = attempts + 1, status = ?2, last_error = ?3 WHERE id = ?1",
                params![id.to_string(), if failed { "failed" } else { "pending" }, err_blob],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Puts a failed item back in the queue (after the cause was fixed).
    pub fn outbox_retry(&self, id: Uuid) -> R<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE outbox SET status = 'pending', last_error = NULL WHERE id = ?1", params![id.to_string()])
            .map_err(db_err)?;
        Ok(())
    }

    // ---------- clearing ----------

    fn clear_reference_data(&self) -> R<()> {
        self.conn
            .lock()
            .unwrap()
            .execute_batch(
                "DELETE FROM credentials; DELETE FROM students; DELETE FROM medicines;
                 DELETE FROM meta WHERE key = 'lists_refreshed_at';",
            )
            .map_err(db_err)
    }

    /// On logout: drop cached lists and saved logins. Unsent visits stay.
    pub fn clear_on_logout(&self) -> R<()> {
        self.clear_reference_data()?;
        let _ = self.conn.lock().unwrap().execute_batch("VACUUM;");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn student(first: &str, last: &str, lrn: &str) -> CachedStudent {
        CachedStudent {
            id: Uuid::new_v4(),
            lrn: lrn.into(),
            first_name: first.into(),
            middle_name: None,
            last_name: last.into(),
            grade_level: 7,
            section_name: "Rosal".into(),
        }
    }

    fn visit() -> PendingVisit {
        PendingVisit {
            body: serde_json::json!({ "complaint": "Headache" }),
            visited_at: Utc::now(),
            student_label: "Dela Cruz, Juan".into(),
            complaint: "Headache".into(),
            recorded_by: "Nurse Joy".into(),
        }
    }

    fn creds() -> Credentials {
        Credentials {
            user: UserInfo { id: Uuid::new_v4(), full_name: "Nurse Joy".into(), username: "nurse_joy".into(), role: "staff".into() },
            school: SchoolInfo { id: Uuid::new_v4(), name: "VNHS".into() },
            password_hash: "$argon2id$x".into(),
            saved_at: Utc::now(),
        }
    }

    #[test]
    fn lists_round_trip_and_search_like_the_server() {
        let c = Cache::in_memory([1; 32]);
        let juan = student("Juan", "Dela Cruz", "123456789012");
        let nina = student("Niña", "Peña", "000000000045");
        c.replace_lists(&[juan.clone(), nina.clone()], &[CachedMedicine { id: Uuid::new_v4(), name: "Paracetamol".into(), unit: "tablet".into() }])
            .unwrap();
        assert_eq!(c.search_students("dela cruz, juan", 10).unwrap(), vec![juan.clone()]);
        assert_eq!(c.search_students("juan dela", 10).unwrap(), vec![juan.clone()]);
        assert_eq!(c.search_students("NIÑA", 10).unwrap(), vec![nina.clone()]);
        assert_eq!(c.search_students("0000", 10).unwrap(), vec![nina.clone()]);
        assert!(c.search_students("zzz", 10).unwrap().is_empty());
        assert!(c.search_students("   ", 10).unwrap().is_empty());
        assert_eq!(c.student(juan.id).unwrap(), Some(juan));
        assert_eq!(c.medicines().unwrap().len(), 1);
        assert!(c.lists_refreshed_at().unwrap().is_some());
    }

    #[test]
    fn stored_rows_are_encrypted() {
        let c = Cache::in_memory([2; 32]);
        c.replace_lists(&[student("Juan", "Dela Cruz", "123456789012")], &[]).unwrap();
        c.outbox_add(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), &visit()).unwrap();
        c.save_credentials("nurse_joy", &creds()).unwrap();
        let conn = c.conn.lock().unwrap();
        for table in ["students", "outbox", "credentials"] {
            let blob: Vec<u8> = conn.query_row(&format!("SELECT payload FROM {table}"), [], |r| r.get(0)).unwrap();
            for secret in [&b"Juan"[..], b"123456789012", b"Headache", b"argon2"] {
                assert!(!blob.windows(secret.len()).any(|w| w == secret), "{table} leaks plaintext");
            }
        }
    }

    #[test]
    fn outbox_lifecycle() {
        let c = Cache::in_memory([3; 32]);
        let (school, user) = (Uuid::new_v4(), Uuid::new_v4());
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        c.outbox_add(school, user, a, &visit()).unwrap();
        c.outbox_add(school, user, b, &visit()).unwrap();
        assert_eq!(c.outbox(school).unwrap().len(), 2);
        assert!(c.outbox(Uuid::new_v4()).unwrap().is_empty(), "scoped to school");
        c.outbox_note(a, false, Some("network")).unwrap();
        c.outbox_note(b, true, Some("Choose a student from the list.")).unwrap();
        let items = c.outbox(school).unwrap();
        let ia = items.iter().find(|i| i.id == a).unwrap();
        let ib = items.iter().find(|i| i.id == b).unwrap();
        assert_eq!((ia.status.as_str(), ia.attempts), ("pending", 1));
        assert_eq!((ib.status.as_str(), ib.last_error.as_deref()), ("failed", Some("Choose a student from the list.")));
        c.outbox_retry(b).unwrap();
        assert_eq!(c.outbox(school).unwrap().iter().find(|i| i.id == b).unwrap().status, "pending");
        c.outbox_delete(a).unwrap();
        assert_eq!(c.outbox(school).unwrap().len(), 1);
    }

    #[test]
    fn logout_clears_everything_but_unsent_visits() {
        let c = Cache::in_memory([4; 32]);
        let school = Uuid::new_v4();
        c.replace_lists(&[student("Juan", "Dela Cruz", "123456789012")], &[]).unwrap();
        c.save_credentials("Nurse_Joy", &creds()).unwrap();
        assert!(c.credentials("NURSE_JOY").unwrap().is_some(), "usernames are case-insensitive");
        c.outbox_add(school, Uuid::new_v4(), Uuid::new_v4(), &visit()).unwrap();
        c.clear_on_logout().unwrap();
        assert!(c.search_students("juan", 10).unwrap().is_empty());
        assert!(c.credentials("nurse_joy").unwrap().is_none());
        assert!(c.lists_refreshed_at().unwrap().is_none());
        assert_eq!(c.outbox(school).unwrap().len(), 1, "unsent visits survive logout");
    }

    #[test]
    fn switching_school_drops_the_old_schools_data() {
        let c = Cache::in_memory([5; 32]);
        c.ensure_school(Uuid::new_v4()).unwrap();
        c.replace_lists(&[student("Juan", "Dela Cruz", "123456789012")], &[]).unwrap();
        c.save_credentials("nurse_joy", &creds()).unwrap();
        c.ensure_school(Uuid::new_v4()).unwrap();
        assert!(c.search_students("juan", 10).unwrap().is_empty());
        assert!(c.credentials("nurse_joy").unwrap().is_none());
    }

    #[test]
    fn opening_with_another_key_cannot_read_rows() {
        let dir = std::env::temp_dir().join(format!("clinilog-cache-test-{}", Uuid::new_v4()));
        {
            let c = Cache::open(&dir).unwrap();
            c.save_credentials("nurse_joy", &creds()).unwrap();
            assert!(c.credentials("nurse_joy").unwrap().is_some());
        }
        // Same key file: still readable after reopening.
        assert!(Cache::open(&dir).unwrap().credentials("nurse_joy").unwrap().is_some());
        // Key lost/replaced: the old file is reset, not half-read.
        std::fs::write(dir.join("offline-cache.key"), b"not a valid protected key").unwrap();
        assert!(Cache::open(&dir).unwrap().credentials("nurse_joy").unwrap().is_none());
        std::fs::remove_dir_all(dir).ok();
    }
}
