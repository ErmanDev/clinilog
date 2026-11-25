//! Offline-first visits and background sync (spec §8).
//!
//! - `record_visit` saves the visit to the encrypted outbox first, then tries to
//!   upload it right away. If the server can't be reached it stays `pending`.
//! - A background loop checks the connection every 30 s (or when woken), uploads
//!   pending visits for the logged-in user, and refreshes the cached student and
//!   medicine lists every 15 minutes.
//! - Each visit has a client-made id, so an upload that's retried after a dropped
//!   connection never creates a duplicate (the server returns the existing visit).
//! - The webview hears about changes through the `sync-status` event.

use chrono::{DateTime, Duration, Utc};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::{
    api::CmdError,
    cache::{CachedMedicine, CachedStudent, PendingVisit},
    AppState,
};

const LOOP_SECS: u64 = 30;
const LIST_REFRESH_MINUTES: i64 = 15;
const STUDENT_PAGE: i64 = 100;

#[derive(Serialize, Clone)]
pub struct SyncStatus {
    /// The server answered the last health check.
    pub online: bool,
    /// The user logged in without the server (offline login) and hasn't reconnected yet.
    pub session_offline: bool,
    /// This user's visits waiting to upload / refused by the server.
    pub pending: usize,
    pub failed: usize,
    /// Visits saved by other users on this PC; they upload when that user logs in.
    pub others_waiting: usize,
    pub lists_refreshed_at: Option<DateTime<Utc>>,
    /// The saved password no longer works (changed or account disabled): log in again.
    pub auth_required: bool,
    pub cache_error: Option<String>,
}

pub fn status(state: &AppState) -> SyncStatus {
    let session = state.session.lock().unwrap();
    let (user_id, school_id, session_offline, auth_required) = match session.as_ref() {
        Some(s) => (Some(s.user.id), Some(s.school.id), s.token.is_none(), s.auth_failed),
        None => (None, state.school_id(), false, false),
    };
    drop(session);
    let (mut pending, mut failed, mut others, mut refreshed) = (0, 0, 0, None);
    if let (Some(cache), Some(school)) = (state.cache.as_ref(), school_id) {
        for item in cache.outbox(school).unwrap_or_default() {
            match (Some(item.user_id) == user_id, item.status.as_str()) {
                (true, "failed") => failed += 1,
                (true, _) => pending += 1,
                (false, _) => others += 1,
            }
        }
        refreshed = cache.lists_refreshed_at().ok().flatten();
    }
    SyncStatus {
        online: state.online.load(Ordering::Relaxed),
        session_offline,
        pending,
        failed,
        others_waiting: others,
        lists_refreshed_at: refreshed,
        auth_required,
        cache_error: state.cache_error.clone(),
    }
}

/// Records the result of any server call, so the online indicator changes as soon as
/// a request fails or succeeds instead of waiting for the next background check.
pub(crate) fn note_connection(app: &AppHandle, reachable: bool) {
    let state = app.state::<AppState>();
    if state.online.swap(reachable, Ordering::Relaxed) != reachable {
        emit(app);
    }
}

pub(crate) fn emit(app: &AppHandle) {
    let state = app.state::<AppState>();
    let _ = app.emit("sync-status", status(&state));
}

/// Background loop, started once at app launch.
pub async fn run(app: AppHandle) {
    loop {
        sync_once(&app).await;
        emit(&app);
        let state = app.state::<AppState>();
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(LOOP_SECS)) => {}
            _ = state.sync_wake.notified() => {}
        }
    }
}

/// One pass: connection check → upload this user's pending visits → refresh lists if stale.
/// Runs one at a time (a second caller waits) so uploads never overlap.
pub async fn sync_once(app: &AppHandle) {
    let state = app.state::<AppState>();
    let _guard = state.sync_lock.lock().await;
    let Ok(api) = state.api() else { return };

    let online = api.health_quick().await.is_ok();
    state.online.store(online, Ordering::Relaxed);
    let skip = state.session.lock().unwrap().as_ref().is_none_or(|s| s.is_superadmin());
    if !online || skip {
        return;
    }
    let Ok(token) = state.ensure_token().await else { return };
    let (user_id, school_id) = match state.session.lock().unwrap().as_ref() {
        Some(s) => (s.user.id, s.school.id),
        None => return,
    };

    if let Some(cache) = state.cache.as_ref() {
        let mine: Vec<_> = cache
            .outbox(school_id)
            .unwrap_or_default()
            .into_iter()
            .filter(|i| i.user_id == user_id && i.status == "pending")
            .collect();
        for item in mine {
            match upload(&state, &token, &item.visit).await {
                Upload::Done => {
                    let _ = cache.outbox_delete(item.id);
                }
                Upload::Retry(msg) => {
                    let _ = cache.outbox_note(item.id, false, Some(&msg));
                    break; // connection or account problem: stop for now
                }
                Upload::Refused(e) => {
                    let _ = cache.outbox_note(item.id, true, Some(&refusal_text(&e)));
                }
            }
        }

        let stale = cache
            .lists_refreshed_at()
            .ok()
            .flatten()
            .map_or(true, |t| Utc::now() - t > Duration::minutes(LIST_REFRESH_MINUTES));
        if stale {
            if let Err(e) = refresh_lists(&state, &token).await {
                eprintln!("offline cache: couldn't refresh lists: {}", e.message);
            }
        }
    }
}

enum Upload {
    Done,
    /// Try again later (no connection, server busy, subscription inactive, session expired).
    Retry(String),
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

async fn upload(state: &AppState, token: &str, visit: &PendingVisit) -> Upload {
    let Ok(api) = state.api() else { return Upload::Retry("App is not configured.".into()) };
    match api.request(token, Method::POST, "visits", Some(visit.body.clone())).await {
        Ok(_) => Upload::Done,
        Err(e) => match e.kind {
            "network" | "server" | "forbidden" => Upload::Retry(e.message),
            "unauthorized" => {
                state.drop_token();
                Upload::Retry(e.message)
            }
            _ => Upload::Refused(e),
        },
    }
}

async fn refresh_lists(state: &AppState, token: &str) -> Result<(), CmdError> {
    let api = state.api()?;
    let mut students = Vec::new();
    for page in 1.. {
        let res = api
            .request(token, Method::GET, &format!("students?page={page}&page_size={STUDENT_PAGE}"), None)
            .await?
            .unwrap_or_default();
        let items = res["items"].as_array().cloned().unwrap_or_default();
        let n = items.len() as i64;
        students.extend(items.iter().filter_map(|s| {
            Some(CachedStudent {
                id: s["id"].as_str()?.parse().ok()?,
                lrn: s["lrn"].as_str()?.to_string(),
                first_name: s["first_name"].as_str()?.to_string(),
                middle_name: s["middle_name"].as_str().map(str::to_string),
                last_name: s["last_name"].as_str()?.to_string(),
                grade_level: s["section"]["grade_level"].as_i64()? as i32,
                section_name: s["section"]["section_name"].as_str()?.to_string(),
            })
        }));
        if n < STUDENT_PAGE {
            break;
        }
    }
    let meds = api.request(token, Method::GET, "medicines", None).await?.unwrap_or_default();
    let medicines: Vec<CachedMedicine> = meds
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|m| {
            Some(CachedMedicine {
                id: m["id"].as_str()?.parse().ok()?,
                name: m["name"].as_str()?.to_string(),
                unit: m["unit"].as_str()?.to_string(),
            })
        })
        .collect();
    if let Some(cache) = state.cache.as_ref() {
        cache.replace_lists(&students, &medicines).map_err(|e| CmdError::new("server", e))?;
    }
    Ok(())
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
    /// "Dela Cruz, Juan" — shown in the waiting list while offline.
    pub student_label: String,
}

#[derive(Serialize)]
pub struct RecordResult {
    /// `synced` = on the server now; `pending` = saved on this PC, uploads later.
    pub status: &'static str,
    pub visit_id: Uuid,
}

/// Same limits the server checks, so a visit saved offline won't be refused later.
fn check_draft(d: &VisitDraft) -> Result<(), CmdError> {
    let mut fields = serde_json::Map::new();
    let len = |s: &str| s.trim().chars().count();
    match len(&d.complaint) {
        0 => fields.insert("complaint".into(), json!("Enter the complaint.")),
        1..=2 => fields.insert("complaint".into(), json!("Complaint must be at least 3 characters.")),
        n if n > 500 => fields.insert("complaint".into(), json!("Complaint must be 500 characters or fewer.")),
        _ => None,
    };
    match len(&d.treatment) {
        0 => fields.insert("treatment".into(), json!("Enter the treatment.")),
        n if n > 500 => fields.insert("treatment".into(), json!("Treatment must be 500 characters or fewer.")),
        _ => None,
    };
    let mut seen = std::collections::HashSet::new();
    for (i, m) in d.medicines.iter().enumerate() {
        if !(1..=100).contains(&m.quantity) {
            fields.insert(format!("medicines.{i}.quantity"), json!("Quantity must be from 1 to 100."));
        }
        if !seen.insert(m.medicine_id) {
            fields.insert(format!("medicines.{i}.medicine_id"), json!("This medicine is already listed."));
        }
    }
    if d.medicines.len() > 20 {
        fields.insert("medicines".into(), json!("A visit can list up to 20 medicines."));
    }
    if fields.is_empty() {
        Ok(())
    } else {
        Err(CmdError { fields: Some(fields), ..CmdError::new("invalid", "Please fix the highlighted fields.") })
    }
}

/// Saves a new visit on this PC first, then uploads it if the server is reachable.
#[tauri::command]
pub async fn record_visit(app: AppHandle, state: State<'_, AppState>, draft: VisitDraft) -> Result<RecordResult, CmdError> {
    check_draft(&draft)?;
    let (user_id, school_id, recorded_by) = state
        .session
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| (s.user.id, s.school.id, s.user.full_name.clone()))
        .ok_or_else(CmdError::unauthorized)?;
    let cache = state
        .cache
        .as_ref()
        .ok_or_else(|| CmdError::new("server", state.cache_error.clone().unwrap_or_else(|| "Offline storage is unavailable.".into())))?;

    let id = Uuid::new_v4();
    let visited_at = Utc::now();
    let body = json!({
        "id": id,
        "student_id": draft.student_id,
        "complaint": draft.complaint,
        "treatment": draft.treatment,
        "medicines": draft.medicines.iter().map(|m| json!({ "medicine_id": m.medicine_id, "quantity": m.quantity })).collect::<Vec<_>>(),
        "visited_at": visited_at,
    });
    let visit = PendingVisit {
        body,
        visited_at,
        student_label: draft.student_label.clone(),
        complaint: draft.complaint.trim().to_string(),
        recorded_by,
    };
    cache.outbox_add(school_id, user_id, id, &visit).map_err(|e| CmdError::new("server", e))?;

    // Already known to be offline: don't make the user wait for a connection timeout.
    // The background sync uploads it as soon as the server answers again.
    if !state.online.load(Ordering::Relaxed) {
        state.sync_wake.notify_one();
        emit(&app);
        return Ok(RecordResult { status: "pending", visit_id: id });
    }

    // Try to upload now. Only a missing connection leaves it pending; anything the
    // server refuses goes straight back to the form so the user can fix it.
    let result = match state.ensure_token().await {
        Err(_) => RecordResult { status: "pending", visit_id: id },
        Ok(token) => match upload(&state, &token, &visit).await {
            Upload::Done => {
                let _ = cache.outbox_delete(id);
                RecordResult { status: "synced", visit_id: id }
            }
            Upload::Retry(_) => RecordResult { status: "pending", visit_id: id },
            Upload::Refused(err) => {
                // The user is still on the form: drop the draft and show what to fix.
                let _ = cache.outbox_delete(id);
                emit(&app);
                return Err(err);
            }
        },
    };
    emit(&app);
    Ok(result)
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
    pub visited_at: DateTime<Utc>,
    pub student_label: String,
    pub complaint: String,
    pub recorded_by: String,
}

/// Visits saved on this PC that haven't reached the server, for this school.
#[tauri::command]
pub fn outbox_list(state: State<'_, AppState>) -> Result<Vec<OutboxView>, CmdError> {
    let (user_id, school_id) = state
        .session
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| (s.user.id, s.school.id))
        .ok_or_else(CmdError::unauthorized)?;
    let Some(cache) = state.cache.as_ref() else { return Ok(vec![]) };
    Ok(cache
        .outbox(school_id)
        .map_err(|e| CmdError::new("server", e))?
        .into_iter()
        .map(|i| OutboxView {
            id: i.id,
            mine: i.user_id == user_id,
            status: i.status,
            attempts: i.attempts,
            last_error: i.last_error,
            visited_at: i.visit.visited_at,
            student_label: i.visit.student_label,
            complaint: i.visit.complaint,
            recorded_by: i.visit.recorded_by,
        })
        .collect())
}

fn own_item(state: &AppState, id: Uuid) -> Result<(), CmdError> {
    let (user_id, school_id) = state
        .session
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| (s.user.id, s.school.id))
        .ok_or_else(CmdError::unauthorized)?;
    let cache = state.cache.as_ref().ok_or_else(|| CmdError::new("server", "Offline storage is unavailable."))?;
    let ok = cache.outbox(school_id).unwrap_or_default().iter().any(|i| i.id == id && i.user_id == user_id);
    if ok {
        Ok(())
    } else {
        Err(CmdError::new("forbidden", "Only the person who recorded this visit can change it."))
    }
}

/// Puts a refused visit back in the queue (e.g. after fixing the student record).
#[tauri::command]
pub async fn outbox_retry(app: AppHandle, state: State<'_, AppState>, id: Uuid) -> Result<(), CmdError> {
    own_item(&state, id)?;
    if let Some(c) = state.cache.as_ref() {
        c.outbox_retry(id).map_err(|e| CmdError::new("server", e))?;
    }
    state.sync_wake.notify_one();
    emit(&app);
    Ok(())
}

/// Deletes a visit that was never uploaded. The UI asks for confirmation first.
#[tauri::command]
pub fn outbox_discard(app: AppHandle, state: State<'_, AppState>, id: Uuid) -> Result<(), CmdError> {
    own_item(&state, id)?;
    if let Some(c) = state.cache.as_ref() {
        c.outbox_delete(id).map_err(|e| CmdError::new("server", e))?;
    }
    emit(&app);
    Ok(())
}

/// Student search from the cache (used when the server can't be reached).
#[tauri::command]
pub fn cached_students(state: State<'_, AppState>, q: String) -> Result<Vec<CachedStudent>, CmdError> {
    state.session.lock().unwrap().as_ref().ok_or_else(CmdError::unauthorized)?;
    match state.cache.as_ref() {
        Some(c) => c.search_students(&q, 8).map_err(|e| CmdError::new("server", e)),
        None => Ok(vec![]),
    }
}

#[tauri::command]
pub fn cached_student(state: State<'_, AppState>, id: Uuid) -> Result<Option<CachedStudent>, CmdError> {
    state.session.lock().unwrap().as_ref().ok_or_else(CmdError::unauthorized)?;
    match state.cache.as_ref() {
        Some(c) => c.student(id).map_err(|e| CmdError::new("server", e)),
        None => Ok(None),
    }
}

#[tauri::command]
pub fn cached_medicines(state: State<'_, AppState>) -> Result<Vec<CachedMedicine>, CmdError> {
    state.session.lock().unwrap().as_ref().ok_or_else(CmdError::unauthorized)?;
    match state.cache.as_ref() {
        Some(c) => c.medicines().map_err(|e| CmdError::new("server", e)),
        None => Ok(vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(complaint: &str, treatment: &str, meds: Vec<(Uuid, i64)>) -> VisitDraft {
        VisitDraft {
            student_id: Uuid::new_v4(),
            complaint: complaint.into(),
            treatment: treatment.into(),
            medicines: meds.into_iter().map(|(medicine_id, quantity)| MedicineLine { medicine_id, quantity }).collect(),
            student_label: "X".into(),
        }
    }

    #[test]
    fn drafts_are_checked_like_the_server() {
        assert!(check_draft(&draft("Headache", "Rest", vec![(Uuid::new_v4(), 1)])).is_ok());
        let m = Uuid::new_v4();
        let err = check_draft(&draft("ab", " ", vec![(m, 0), (m, 101)])).unwrap_err();
        let f = err.fields.unwrap();
        for k in ["complaint", "treatment", "medicines.0.quantity", "medicines.1.quantity", "medicines.1.medicine_id"] {
            assert!(f.contains_key(k), "missing {k}");
        }
        assert!(check_draft(&draft(&"x".repeat(501), "ok", vec![])).is_err());
    }
}
