//! Working offline and syncing (spec §8).
//!
//! - Every page's request goes through [`request`]. Online, it goes to the server.
//!   When the server can't be reached it is answered from the offline copy
//!   (`local.rs`): reads come from the records saved on this PC, and changes are
//!   checked the way the server checks them, saved in the encrypted outbox and
//!   shown right away.
//! - A background loop checks the connection every 30 s (10 s while offline), or
//!   when woken. Once the server answers it uploads this user's waiting changes in
//!   the order they were made, then the records they opened offline (for the audit
//!   trail), then downloads what changed on the server since the last sync.
//! - New records get an id made on this PC, so an upload that's retried after a
//!   dropped connection never creates a duplicate (the server returns the existing one).
//! - The webview hears about changes through the `sync-status` event.

use chrono::{DateTime, Duration, Utc};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::{
    api::CmdError,
    cache::{OutboxItem, PendingOp, ViewEntry},
    local,
    mirror::{Cursors, SyncPage},
    plan, AppState,
};

const LOOP_SECS_ONLINE: u64 = 30;
const LOOP_SECS_OFFLINE: u64 = 10;
const SYNC_PAGE: i64 = 500;
const VIEWS_PER_UPLOAD: i64 = 200;
/// Each sync starts this far before the last position, to catch rows from slow transactions.
const SYNC_OVERLAP_MINUTES: i64 = 5;

#[derive(Serialize, Clone)]
pub struct SyncStatus {
    /// The server answered the last check.
    pub online: bool,
    /// The user logged in without the server (offline login) and hasn't reconnected yet.
    pub session_offline: bool,
    /// This user's changes waiting to upload / refused by the server.
    pub pending: usize,
    pub failed: usize,
    /// Changes saved by other users on this PC; they upload when that user logs in.
    pub others_waiting: usize,
    /// When the records saved on this PC were last updated from the server.
    pub synced_at: Option<DateTime<Utc>>,
    /// The saved password no longer works (changed or account disabled): log in again.
    pub auth_required: bool,
    pub cache_error: Option<String>,
}

/// The logged-in user, as far as offline work needs to know.
struct Who {
    user_id: Uuid,
    name: String,
    is_admin: bool,
    school_id: Uuid,
    superadmin: bool,
    has_token: bool,
}

fn who(state: &AppState) -> Option<Who> {
    state.session.lock().unwrap().as_ref().map(|s| Who {
        user_id: s.user.id,
        name: s.user.full_name.clone(),
        is_admin: s.user.role == "admin",
        school_id: s.school.id,
        superadmin: s.is_superadmin(),
        has_token: s.token_present(),
    })
}

pub fn status(state: &AppState) -> SyncStatus {
    let session = state.session.lock().unwrap();
    let (user_id, school_id, session_offline, auth_required) = match session.as_ref() {
        Some(s) => (Some(s.user.id), Some(s.school.id), !s.token_present(), s.auth_failed),
        None => (None, state.school_id(), false, false),
    };
    drop(session);
    let (mut pending, mut failed, mut others, mut synced_at) = (0, 0, 0, None);
    if let (Some(cache), Some(school)) = (state.cache.as_ref(), school_id) {
        for item in cache.outbox(school).unwrap_or_default() {
            match (Some(item.user_id) == user_id, item.status.as_str()) {
                (true, "failed") => failed += 1,
                (true, _) => pending += 1,
                (false, _) => others += 1,
            }
        }
        synced_at = cache.synced_at().ok().flatten();
    }
    SyncStatus {
        online: state.online.load(Ordering::Relaxed),
        session_offline,
        pending,
        failed,
        others_waiting: others,
        synced_at,
        auth_required,
        cache_error: state.cache_error.clone(),
    }
}

/// Records the result of any server call, so the online indicator changes as soon as
/// a request fails or succeeds instead of waiting for the next background check.
pub(crate) fn note_connection(app: &AppHandle, reachable: bool) {
    let state = app.state::<AppState>();
    if state.online.swap(reachable, Ordering::Relaxed) != reachable {
        if reachable {
            // Back online: upload what's waiting without waiting for the next tick.
            state.sync_wake.notify_one();
        }
        emit(app);
    }
}

pub(crate) fn emit(app: &AppHandle) {
    let state = app.state::<AppState>();
    let _ = app.emit("sync-status", status(&state));
}

// ---------- requests from the pages ----------

fn plan_flags(state: &AppState, school_id: Uuid) -> Option<local::PlanFlags> {
    plan::load_cached(state.data_dir.as_deref(), school_id).map(|p| local::PlanFlags {
        advanced_search: p.info.features.advanced_search,
        detailed_reports: p.info.features.detailed_reports,
        read_only: p.info.read_only,
        grace_expired: p.grace_expired,
        max_students: p.info.limits.max_students,
    })
}

fn offline_error() -> CmdError {
    CmdError::new("network", "You're offline, and this needs the Clinilog server. Try again when the internet connection is back.")
}

/// Pages that need the server but can show their last copy while offline.
/// Saved per user, so an admin's pages never show up for someone else.
fn keeps_saved_copy(path: &str) -> bool {
    let route = path.split('?').next().unwrap_or("");
    matches!(route, "users" | "activity" | "subscription/plans" | "subscription/requests")
}

/// POSTs that create a record whose id the app can choose (so a retry can't duplicate it).
fn takes_client_id(path: &str) -> bool {
    matches!(path, "students" | "visits" | "sections" | "medicines")
}

/// This user has changes that haven't uploaded yet (so new changes must queue behind them).
fn has_waiting(state: &AppState, w: &Who) -> bool {
    state
        .cache
        .as_ref()
        .and_then(|c| c.outbox(w.school_id).ok())
        .is_some_and(|items| items.iter().any(|i| i.user_id == w.user_id && i.status == "pending"))
}

/// An API request from a page: to the server when it can be reached, otherwise
/// answered from (or saved to) the offline copy.
pub(crate) async fn request(app: &AppHandle, state: &AppState, method: Method, path: &str, body: Option<Value>) -> Result<Option<Value>, CmdError> {
    let w = who(state).ok_or_else(CmdError::unauthorized)?;
    let cache = state.cache.as_ref().filter(|_| !w.superadmin);
    let has_copy = cache.is_some_and(|c| c.synced_at().ok().flatten().is_some());
    let online = state.online.load(Ordering::Relaxed);

    if method == Method::GET {
        let local_ok = has_copy && local::can_read(path);
        // Offline, or this user's own changes haven't reached the server yet: the copy
        // here is the more complete one.
        if local_ok && (!online || !w.has_token || has_waiting(state, &w)) {
            return local_read(state, &w, path).expect("checked");
        }
        if !online && keeps_saved_copy(path) {
            if let Some(saved) = cache.and_then(|c| c.response(w.user_id, path).ok().flatten()) {
                return Ok(Some(saved));
            }
        }
        return match server(app, state, Method::GET, path, None).await {
            Ok(res) => {
                if let (Some(c), Some(v), true) = (cache, res.as_ref(), keeps_saved_copy(path)) {
                    let _ = c.save_response(w.user_id, path, v);
                }
                Ok(res)
            }
            Err(e) if e.kind == "network" => {
                if local_ok {
                    return local_read(state, &w, path).expect("checked");
                }
                match cache.filter(|_| keeps_saved_copy(path)).and_then(|c| c.response(w.user_id, path).ok().flatten()) {
                    Some(saved) => Ok(Some(saved)),
                    None if cache.is_some() => Err(offline_error()),
                    None => Err(e),
                }
            }
            Err(e) => Err(e),
        };
    }

    // A change. New records get their id here, so the upload is safe to retry.
    let mut body = body;
    if method == Method::POST && takes_client_id(path) {
        let mut b = body.take().unwrap_or_else(|| json!({}));
        if let Some(o) = b.as_object_mut() {
            o.entry("id").or_insert_with(|| json!(Uuid::new_v4()));
        }
        body = Some(b);
    }
    save_change(app, state, &w, method.as_str(), path, body).await.map(|saved| saved.response)
}

/// Answers a GET from the offline copy, noting an opened student or visit for the
/// audit trail. `None` = this page needs the server.
fn local_read(state: &AppState, w: &Who, path: &str) -> Option<Result<Option<Value>, CmdError>> {
    let cache = state.cache.as_ref()?;
    let view = match cache.view() {
        Ok(v) => v,
        Err(e) => return Some(Err(CmdError::new("server", e))),
    };
    let plan = plan_flags(state, w.school_id);
    let ctx = local::Ctx { user_id: w.user_id, user_name: &w.name, is_admin: w.is_admin, plan: plan.as_ref(), now: Utc::now(), replay: false };
    let answer = local::read(&view, &ctx, path)?;
    Some(answer.map(|a| {
        if let Some((table, id)) = a.viewed {
            let _ = cache.log_view(w.school_id, w.user_id, &ViewEntry { table_name: table.into(), record_id: id, viewed_at: Utc::now() });
        }
        Some(a.body)
    }))
}

/// One authenticated call to the server, updating the online indicator.
async fn server(app: &AppHandle, state: &AppState, method: Method, path: &str, body: Option<Value>) -> Result<Option<Value>, CmdError> {
    let result = match state.ensure_token().await {
        Ok(token) => state.api()?.request(&token, method, path, body).await,
        Err(e) => Err(e),
    };
    match &result {
        Err(e) if e.kind == "network" => note_connection(app, false),
        Err(e) if e.kind == "unauthorized" => state.clear_session(),
        _ => note_connection(app, true),
    }
    result
}

pub(crate) struct Saved {
    pub response: Option<Value>,
    /// On the server now (otherwise saved on this PC, uploading later).
    pub uploaded: bool,
}

/// Saves a change. Online with nothing waiting, it goes straight to the server; if the
/// server can't be reached (or this user's earlier changes are still waiting), it is
/// checked against the offline copy and queued.
async fn save_change(app: &AppHandle, state: &AppState, w: &Who, method: &str, path: &str, body: Option<Value>) -> Result<Saved, CmdError> {
    let m = Method::from_bytes(method.as_bytes()).map_err(|_| CmdError::new("invalid", "Unsupported request method."))?;
    let Some(cache) = state.cache.as_ref().filter(|_| !w.superadmin && local::can_write(method, path)) else {
        // Needs the server (user accounts, payments), or there's no offline storage.
        return match server(app, state, m, path, body).await {
            Ok(response) => Ok(Saved { response, uploaded: true }),
            Err(e) if e.kind == "network" && state.cache.is_some() && !w.superadmin => Err(offline_error()),
            Err(e) => Err(e),
        };
    };
    let has_copy = cache.synced_at().ok().flatten().is_some();
    let queue_first = has_copy && has_waiting(state, w);

    if state.online.load(Ordering::Relaxed) && w.has_token && !queue_first {
        match server(app, state, m, path, body.clone()).await {
            Ok(response) => {
                // Keep the offline copy in step until the next sync brings the server's version.
                let _ = cache.accept_direct(w.user_id, &w.name, method, path, body.as_ref());
                state.sync_wake.notify_one();
                return Ok(Saved { response, uploaded: true });
            }
            Err(e) if e.kind == "network" => {} // save it here instead
            Err(e) => return Err(e),
        }
    }

    if !has_copy {
        return Err(CmdError::new(
            "network",
            "You're offline, and this PC doesn't have the school's records yet. Connect once to download them; after that, changes can be saved offline.",
        ));
    }
    let view = cache.view().map_err(|e| CmdError::new("server", e))?;
    let plan = plan_flags(state, w.school_id);
    let ctx = local::Ctx { user_id: w.user_id, user_name: &w.name, is_admin: w.is_admin, plan: plan.as_ref(), now: Utc::now(), replay: false };
    let written = local::write(&view, &ctx, method, path, body.as_ref()).expect("can_write was checked")?;
    let op_id = Uuid::new_v4();
    let op = PendingOp {
        method: method.to_string(),
        path: written.path,
        body: written.body,
        at: ctx.now,
        action: written.action.to_string(),
        label: written.label,
        summary: written.summary,
        recorded_by: w.name.clone(),
    };
    cache.outbox_add(w.school_id, w.user_id, op_id, &op).map_err(|e| CmdError::new("server", e))?;
    emit(app);

    // Online (just behind earlier changes): upload now, so a refusal reaches the form.
    if state.online.load(Ordering::Relaxed) {
        if let Ok(token) = state.ensure_token().await {
            let outcome = upload_waiting(app, state, &token, Some(op_id)).await;
            emit(app);
            match outcome {
                Some(Outcome::Done(response)) => return Ok(Saved { response, uploaded: true }),
                Some(Outcome::Refused(e)) => {
                    // The user is still on the form: drop it there and show what to fix.
                    let _ = cache.outbox_delete(op_id);
                    emit(app);
                    return Err(e);
                }
                _ => {}
            }
        }
    }
    state.sync_wake.notify_one();
    Ok(Saved { response: written.response, uploaded: false })
}

// ---------- background sync ----------

/// Background loop, started once at app launch.
pub async fn run(app: AppHandle) {
    loop {
        sync_once(&app).await;
        emit(&app);
        let state = app.state::<AppState>();
        let wait = if state.online.load(Ordering::Relaxed) { LOOP_SECS_ONLINE } else { LOOP_SECS_OFFLINE };
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(wait)) => {}
            _ = state.sync_wake.notified() => {}
        }
    }
}

/// One pass: connection check → upload this user's changes → upload offline views →
/// download what changed. Runs one at a time (a second caller waits).
pub async fn sync_once(app: &AppHandle) {
    let state = app.state::<AppState>();
    let _guard = state.sync_lock.lock().await;
    let Ok(api) = state.api() else { return };

    let online = api.health_quick().await.is_ok();
    if state.online.swap(online, Ordering::Relaxed) != online {
        emit(app);
    }
    let Some(w) = who(&state) else { return };
    if !online || w.superadmin || state.cache.is_none() {
        return;
    }
    let Ok(token) = state.ensure_token().await else { return };
    emit(app); // a session that logged in offline is now connected

    upload_waiting(app, &state, &token, None).await;
    if let Err(e) = upload_views(&state, &token, &w).await {
        eprintln!("offline sync: couldn't upload views: {}", e.message);
    }
    if let Err(e) = pull(&state, &token).await {
        if e.kind == "unauthorized" {
            state.drop_token();
        }
        eprintln!("offline sync: couldn't download changes: {}", e.message);
    }
}

enum Outcome {
    Done(Option<Value>),
    /// Try again later (no connection, server busy, subscription inactive, session expired).
    /// `offline` = the server couldn't be reached at all.
    Retry { offline: bool },
    /// The server won't accept it as-is; a person has to look at it.
    Refused(CmdError),
}

/// One-line explanation of a refusal, including the first field message if any.
fn refusal_text(e: &CmdError) -> String {
    match e.fields.as_ref().and_then(|f| f.values().next()).and_then(|v| v.as_str()) {
        Some(field_msg) => format!("{} {}", e.message, field_msg),
        None => e.message.clone(),
    }
}

async fn upload(state: &AppState, token: &str, op: &PendingOp) -> Outcome {
    let Ok(api) = state.api() else { return Outcome::Retry { offline: false } };
    let Ok(method) = Method::from_bytes(op.method.as_bytes()) else {
        return Outcome::Refused(CmdError::new("invalid", "Unknown change."));
    };
    match api.request(token, method.clone(), &op.path, op.body.clone()).await {
        Ok(res) => Outcome::Done(res),
        // Deleting something that's already gone did what was asked.
        Err(e) if e.kind == "not_found" && method == Method::DELETE => Outcome::Done(None),
        Err(e) => match e.kind {
            "network" => Outcome::Retry { offline: true },
            "server" => Outcome::Retry { offline: false },
            "forbidden" if e.code.as_deref() == Some("read_only") => Outcome::Retry { offline: false },
            "unauthorized" => {
                state.drop_token();
                Outcome::Retry { offline: false }
            }
            _ => Outcome::Refused(e),
        },
    }
}

/// Uploads this user's waiting changes in the order they were made. Stops at the first
/// one that has to wait (keeping the order); refused ones are set aside for a person.
/// Returns what happened to `watch`, if it was reached.
async fn upload_waiting(app: &AppHandle, state: &AppState, token: &str, watch: Option<Uuid>) -> Option<Outcome> {
    let _uploading = state.upload_lock.lock().await;
    let w = who(state)?;
    let cache = state.cache.as_ref()?;
    let mine: Vec<OutboxItem> = cache
        .outbox(w.school_id)
        .unwrap_or_default()
        .into_iter()
        .filter(|i| i.user_id == w.user_id && i.status == "pending")
        .collect();
    let mut watched = None;
    for item in mine {
        let outcome = upload(state, token, &item.op).await;
        match &outcome {
            Outcome::Done(_) => {
                let _ = cache.accept(&item);
            }
            Outcome::Retry { offline } => {
                let _ = cache.outbox_note(item.id, false, None);
                if *offline {
                    note_connection(app, false);
                }
            }
            Outcome::Refused(e) => {
                let _ = cache.outbox_note(item.id, true, Some(&refusal_text(e)));
            }
        }
        emit(app);
        let stop = matches!(outcome, Outcome::Retry { .. });
        if Some(item.id) == watch {
            watched = Some(outcome);
        }
        if stop {
            break;
        }
    }
    watched
}

/// Uploads the records this user opened while offline, so the audit trail is complete.
async fn upload_views(state: &AppState, token: &str, w: &Who) -> Result<(), CmdError> {
    let Some(cache) = state.cache.as_ref() else { return Ok(()) };
    loop {
        let batch = cache.views(w.school_id, w.user_id, VIEWS_PER_UPLOAD).map_err(|e| CmdError::new("server", e))?;
        if batch.is_empty() {
            return Ok(());
        }
        let views: Vec<Value> = batch
            .iter()
            .map(|(id, v)| json!({ "id": id, "table_name": v.table_name, "record_id": v.record_id, "viewed_at": v.viewed_at }))
            .collect();
        match state.api()?.request(token, Method::POST, "activity/views", Some(json!({ "views": views }))).await {
            Ok(_) => {}
            // Something in the batch the server can't use; don't let it block the rest forever.
            Err(e) if e.kind == "invalid" => {}
            Err(e) => return Err(e),
        }
        let ids: Vec<Uuid> = batch.iter().map(|(id, _)| *id).collect();
        cache.views_delete(&ids).map_err(|e| CmdError::new("server", e))?;
        if (ids.len() as i64) < VIEWS_PER_UPLOAD {
            return Ok(());
        }
    }
}

/// A cursor moved back by the overlap, or `None` to start over if it can't be read.
fn rewind(cursor: &Option<String>) -> Option<String> {
    let (at, _) = cursor.as_deref()?.split_once('|')?;
    let at: DateTime<Utc> = DateTime::parse_from_rfc3339(at).ok()?.with_timezone(&Utc);
    Some(format!("{}|{}", (at - Duration::minutes(SYNC_OVERLAP_MINUTES)).to_rfc3339_opts(chrono::SecondsFormat::Micros, true), Uuid::nil()))
}

/// Downloads everything that changed on the server since the last sync (or everything,
/// the first time) into the offline copy.
async fn pull(state: &AppState, token: &str) -> Result<(), CmdError> {
    let Some(cache) = state.cache.as_ref() else { return Ok(()) };
    let api = state.api()?;
    let (saved, epoch) = cache.sync_start().map_err(|e| CmdError::new("server", e))?;
    let fresh = saved.is_none();
    let mut cur = saved
        .map(|c| Cursors { students: rewind(&c.students), visits: rewind(&c.visits), deleted: rewind(&c.deleted) })
        .unwrap_or_default();
    let mut first = true;
    loop {
        let mut url = format!("sync?limit={SYNC_PAGE}");
        for (k, v) in [("students", &cur.students), ("visits", &cur.visits), ("deleted", &cur.deleted)] {
            if let Some(v) = v {
                url.push_str(&format!("&{k}={}", url::form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()));
            }
        }
        let res = api.request(token, Method::GET, &url, None).await?.unwrap_or_default();
        let page: SyncPage =
            serde_json::from_value(res).map_err(|e| CmdError::new("server", format!("The server sent sync data the app didn't understand: {e}")))?;
        let more = page.more;
        cur = page.next.clone();
        cache.apply_sync_page(page, fresh && first, epoch).map_err(|e| CmdError::new("server", e))?;
        first = false;
        if !more {
            return Ok(());
        }
    }
}

// ---------- commands ----------

#[derive(Deserialize)]
pub struct MedicineLine {
    pub medicine_id: Uuid,
    pub quantity: i64,
}

#[derive(Deserialize)]
pub struct VisitDraft {
    pub student_id: Uuid,
    pub complaint: String,
    pub treatment: String,
    #[serde(default)]
    pub medicines: Vec<MedicineLine>,
}

#[derive(Serialize)]
pub struct RecordResult {
    /// `synced` = on the server now; `pending` = saved on this PC, uploads later.
    pub status: &'static str,
    pub visit_id: Uuid,
}

/// Records a new visit: on the server if it can be reached, otherwise saved on this PC.
#[tauri::command]
pub async fn record_visit(app: AppHandle, state: State<'_, AppState>, draft: VisitDraft) -> Result<RecordResult, CmdError> {
    let w = who(&state).ok_or_else(CmdError::unauthorized)?;
    let id = Uuid::new_v4();
    let body = json!({
        "id": id,
        "student_id": draft.student_id,
        "complaint": draft.complaint,
        "treatment": draft.treatment,
        "medicines": draft.medicines.iter().map(|m| json!({ "medicine_id": m.medicine_id, "quantity": m.quantity })).collect::<Vec<_>>(),
    });
    let saved = save_change(&app, &state, &w, "POST", "visits", Some(body)).await?;
    Ok(RecordResult { status: if saved.uploaded { "synced" } else { "pending" }, visit_id: id })
}

#[tauri::command]
pub fn sync_status(state: State<'_, AppState>) -> SyncStatus {
    status(&state)
}

/// Runs a sync pass now (e.g. the "Sync now" button) and returns the result.
#[tauri::command]
pub async fn sync_now(app: AppHandle) -> Result<SyncStatus, CmdError> {
    sync_once(&app).await;
    emit(&app);
    Ok(status(&app.state::<AppState>()))
}

#[derive(Serialize)]
pub struct OutboxView {
    pub id: Uuid,
    pub mine: bool,
    pub status: String,
    pub attempts: i64,
    pub last_error: Option<String>,
    /// When the change was made.
    pub at: DateTime<Utc>,
    pub action: String,
    pub label: String,
    pub summary: String,
    pub recorded_by: String,
}

/// Changes saved on this PC that haven't reached the server, for this school.
#[tauri::command]
pub fn outbox_list(state: State<'_, AppState>) -> Result<Vec<OutboxView>, CmdError> {
    let w = who(&state).ok_or_else(CmdError::unauthorized)?;
    let Some(cache) = state.cache.as_ref() else { return Ok(vec![]) };
    Ok(cache
        .outbox(w.school_id)
        .map_err(|e| CmdError::new("server", e))?
        .into_iter()
        .map(|i| OutboxView {
            id: i.id,
            mine: i.user_id == w.user_id,
            status: i.status,
            attempts: i.attempts,
            last_error: i.last_error,
            at: i.op.at,
            action: i.op.action,
            label: i.op.label,
            summary: i.op.summary,
            recorded_by: i.op.recorded_by,
        })
        .collect())
}

fn own_item(state: &AppState, id: Uuid) -> Result<OutboxItem, CmdError> {
    let w = who(state).ok_or_else(CmdError::unauthorized)?;
    let cache = state.cache.as_ref().ok_or_else(|| CmdError::new("server", "Offline storage is unavailable."))?;
    cache
        .outbox(w.school_id)
        .unwrap_or_default()
        .into_iter()
        .find(|i| i.id == id && i.user_id == w.user_id)
        .ok_or_else(|| CmdError::new("forbidden", "Only the person who made this change can retry or discard it."))
}

/// Puts a refused change back in the queue (e.g. after fixing the student record).
/// For a student, trying again also confirms "save anyway" after a possible-duplicate warning.
#[tauri::command]
pub async fn outbox_retry(app: AppHandle, state: State<'_, AppState>, id: Uuid) -> Result<(), CmdError> {
    let item = own_item(&state, id)?;
    let mut op = item.op.clone();
    let changed = op.path.starts_with("students") && op.method != "DELETE";
    if changed {
        if let Some(o) = op.body.as_mut().and_then(Value::as_object_mut) {
            o.insert("allow_duplicate".into(), json!(true));
        }
    }
    if let Some(c) = state.cache.as_ref() {
        c.outbox_retry(id, changed.then_some(&op)).map_err(|e| CmdError::new("server", e))?;
    }
    state.sync_wake.notify_one();
    emit(&app);
    Ok(())
}

/// Deletes a change that was never uploaded. The UI asks for confirmation first.
#[tauri::command]
pub fn outbox_discard(app: AppHandle, state: State<'_, AppState>, id: Uuid) -> Result<(), CmdError> {
    own_item(&state, id)?;
    if let Some(c) = state.cache.as_ref() {
        c.outbox_delete(id).map_err(|e| CmdError::new("server", e))?;
    }
    emit(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewind_moves_back_and_restarts_on_garbage() {
        let c = Some(format!("2026-11-26T03:10:00.000000Z|{}", Uuid::new_v4()));
        assert_eq!(rewind(&c).unwrap(), format!("2026-11-26T03:05:00.000000Z|{}", Uuid::nil()));
        assert!(rewind(&None).is_none());
        assert!(rewind(&Some("nope".into())).is_none());
    }

    #[test]
    fn saved_copies_and_client_ids_only_where_meant() {
        assert!(keeps_saved_copy("activity?page=2") && keeps_saved_copy("users"));
        assert!(!keeps_saved_copy("students") && !keeps_saved_copy("superadmin/requests"));
        assert!(takes_client_id("visits") && !takes_client_id("users") && !takes_client_id("visits/abc"));
    }
}
