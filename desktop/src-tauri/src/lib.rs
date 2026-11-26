mod api;
mod cache;
mod config;
mod crypto;
mod local;
mod mirror;
mod pdf;
mod plan;
mod sync;

use api::{Api, CmdError, SchoolInfo, SubscriptionInfo, UserInfo};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Mutex},
    time::{Duration, Instant},
};
use tauri::{Manager, State};

/// Offline login lockout, mirroring the server: 5 wrong passwords → 5 minutes.
const OFFLINE_MAX_FAILURES: u32 = 5;
const OFFLINE_LOCK: Duration = Duration::from_secs(5 * 60);

/// Logged-in session. Held in memory only and gone when the app closes.
pub(crate) struct Session {
    /// `None` after an offline login, or once the token expires; renewed from `password`.
    token: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    pub(crate) user: UserInfo,
    pub(crate) school: SchoolInfo,
    /// Kept in memory only, so the app can log back in by itself when the
    /// connection returns or the token expires. Cleared on logout and exit.
    password: String,
    /// The saved password stopped working (changed or account disabled).
    pub(crate) auth_failed: bool,
}

impl Session {
    /// The Clinilog superadmin only reviews payments: no clinic data is cached or synced.
    pub(crate) fn is_superadmin(&self) -> bool {
        self.user.role == "superadmin"
    }

    /// Connected to the server (false after an offline login until the connection returns).
    pub(crate) fn token_present(&self) -> bool {
        self.token.is_some()
    }
}

pub(crate) struct AppState {
    pub(crate) config: Result<config::AppConfig, String>,
    /// The school this PC is connected to: built in, or chosen with a school code.
    pub(crate) school: Mutex<Option<config::SchoolRef>>,
    config_dir: Option<PathBuf>,
    api: Option<Api>,
    data_dir: Option<PathBuf>,
    pub(crate) session: Mutex<Option<Session>>,
    pub(crate) cache: Option<cache::Cache>,
    pub(crate) cache_error: Option<String>,
    pub(crate) online: AtomicBool,
    pub(crate) sync_wake: tokio::sync::Notify,
    pub(crate) sync_lock: tokio::sync::Mutex<()>,
    /// Held while changes upload, so they always go up one at a time and in order.
    pub(crate) upload_lock: tokio::sync::Mutex<()>,
    offline_failures: Mutex<(u32, Option<Instant>)>,
}

impl AppState {
    pub(crate) fn api(&self) -> Result<&Api, CmdError> {
        match (&self.api, &self.config) {
            (Some(api), _) => Ok(api),
            (None, Err(msg)) => Err(CmdError::new("config", msg.clone())),
            (None, Ok(_)) => Err(CmdError::new("config", "App is not configured.")),
        }
    }

    pub(crate) fn school_id(&self) -> Option<uuid::Uuid> {
        self.school.lock().unwrap().as_ref().map(|s| s.id)
    }

    /// A valid access token, logging back in with the remembered password if needed.
    /// `network` = server unreachable (keep working offline); `unauthorized` = log in again.
    pub(crate) async fn ensure_token(&self) -> Result<String, CmdError> {
        let (school_id, username, password) = {
            let guard = self.session.lock().unwrap();
            let s = guard.as_ref().ok_or_else(CmdError::unauthorized)?;
            if let (Some(t), Some(exp)) = (&s.token, s.expires_at) {
                if exp > Utc::now() + chrono::Duration::seconds(30) {
                    return Ok(t.clone());
                }
            }
            if s.auth_failed {
                return Err(auth_changed());
            }
            // The superadmin's session has no school (nil id).
            (Some(s.school.id).filter(|id| !id.is_nil()), s.user.username.clone(), s.password.clone())
        };
        match self.api()?.login(school_id, &username, &password).await {
            Ok(res) => {
                if let Some(s) = self.session.lock().unwrap().as_mut() {
                    s.token = Some(res.token.clone());
                    s.expires_at = Some(res.expires_at);
                    s.user = res.user;
                }
                Ok(res.token)
            }
            Err(e) if e.kind == "network" => Err(e),
            Err(_) => {
                if let Some(s) = self.session.lock().unwrap().as_mut() {
                    s.auth_failed = true;
                    s.token = None;
                }
                Err(auth_changed())
            }
        }
    }

    pub(crate) fn drop_token(&self) {
        if let Some(s) = self.session.lock().unwrap().as_mut() {
            s.token = None;
        }
    }

    fn clear_session(&self) {
        *self.session.lock().unwrap() = None;
    }
}

fn auth_changed() -> CmdError {
    CmdError::new(
        "unauthorized",
        "Your password or account changed since you logged in. Please log in again. Visits saved on this PC are kept.",
    )
}

#[derive(Serialize)]
struct SessionInfo {
    user: UserInfo,
    school: SchoolInfo,
    /// Logged in without the server (offline login).
    offline: bool,
}

#[tauri::command]
fn app_config(state: State<'_, AppState>) -> config::ConfigInfo {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let school = state.school.lock().unwrap().clone();
    match &state.config {
        Ok(c) => config::ConfigInfo {
            api_url: c.api_url.to_string(),
            school_configured: school.is_some(),
            school,
            school_locked: c.school_locked,
            problem: None,
            version,
        },
        Err(msg) => config::ConfigInfo {
            api_url: String::new(),
            school_configured: false,
            school: None,
            school_locked: false,
            problem: Some(msg.clone()),
            version,
        },
    }
}

/// Connects this PC to a school by its code (e.g. VNHS) and remembers it.
#[tauri::command]
async fn set_school(state: State<'_, AppState>, code: String) -> Result<config::SchoolRef, CmdError> {
    if state.config.as_ref().is_ok_and(|c| c.school_locked) {
        return Err(CmdError::new("invalid", "This installation is set up for one school and can't be changed."));
    }
    if code.trim().is_empty() {
        return Err(CmdError::new("invalid", "Enter your school code."));
    }
    let school = state.api()?.lookup_school(&code).await?;
    if let Some(dir) = state.config_dir.as_deref() {
        config::save_school(dir, Some(&school)).map_err(|m| CmdError::new("server", m))?;
    }
    if let Some(c) = state.cache.as_ref() {
        let _ = c.ensure_school(school.id);
    }
    *state.school.lock().unwrap() = Some(school.clone());
    Ok(school)
}

/// Disconnects this PC from its school, so a different school code can be entered.
/// Refused while visits recorded on this PC haven't been uploaded yet.
#[tauri::command]
fn clear_school(state: State<'_, AppState>) -> Result<(), CmdError> {
    if state.config.as_ref().is_ok_and(|c| c.school_locked) {
        return Err(CmdError::new("invalid", "This installation is set up for one school and can't be changed."));
    }
    if state.session.lock().unwrap().is_some() {
        return Err(CmdError::new("invalid", "Log out first."));
    }
    if let (Some(c), Some(id)) = (state.cache.as_ref(), state.school_id()) {
        let waiting = c.outbox(id).map(|o| o.len()).unwrap_or(0);
        if waiting > 0 {
            return Err(CmdError::new(
                "invalid",
                format!(
                    "{waiting} visit{} recorded on this PC still need{} to upload. Log in to this school while online first.",
                    if waiting == 1 { "" } else { "s" },
                    if waiting == 1 { "s" } else { "" }
                ),
            ));
        }
    }
    if let Some(dir) = state.config_dir.as_deref() {
        config::save_school(dir, None).map_err(|m| CmdError::new("server", m))?;
    }
    *state.school.lock().unwrap() = None;
    Ok(())
}

#[tauri::command]
async fn check_server(state: State<'_, AppState>) -> Result<(), CmdError> {
    state.api()?.health().await
}

async fn hash_password(password: String) -> Option<String> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .ok()
            .map(|h| h.to_string())
    })
    .await
    .ok()
    .flatten()
}

async fn verify_password(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash)
            .map(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

/// Logs in with the server; if it can't be reached, falls back to an offline login
/// for an account that logged in on this PC before and hasn't logged out.
#[tauri::command]
async fn login(app: tauri::AppHandle, state: State<'_, AppState>, username: String, password: String) -> Result<SessionInfo, CmdError> {
    let api = state.api()?;
    // No school yet: only the superadmin can log in (the server checks).
    let school_id = state.school_id();
    let username = username.trim().to_string();
    if username.is_empty() || password.is_empty() {
        return Err(CmdError::new("invalid", "Enter your username and password."));
    }
    if let (Some(c), Some(id)) = (state.cache.as_ref(), school_id) {
        let _ = c.ensure_school(id);
    }

    match api.login(school_id, &username, &password).await {
        Ok(res) => {
            // Remember a password check value (never the password) for offline login.
            // Not for the superadmin: that account only works online.
            let superadmin = res.user.role == "superadmin";
            if let (false, Some(c), Some(hash)) = (superadmin, state.cache.as_ref(), hash_password(password.clone()).await) {
                let _ = c.save_credentials(
                    &username,
                    &cache::Credentials { user: res.user.clone(), school: res.school.clone(), password_hash: hash, saved_at: Utc::now() },
                );
            }
            *state.offline_failures.lock().unwrap() = (0, None);
            let info = SessionInfo { user: res.user.clone(), school: res.school.clone(), offline: false };
            *state.session.lock().unwrap() = Some(Session {
                token: Some(res.token),
                expires_at: Some(res.expires_at),
                user: res.user,
                school: res.school,
                password,
                auth_failed: false,
            });
            state.online.store(true, std::sync::atomic::Ordering::Relaxed);
            state.sync_wake.notify_one();
            Ok(info)
        }
        Err(e) if e.kind == "network" => match school_id {
            Some(id) => offline_login(&app, &state, id, &username, password).await,
            None => Err(e),
        },
        Err(e) => Err(e),
    }
}

async fn offline_login(
    app: &tauri::AppHandle,
    state: &AppState,
    school_id: uuid::Uuid,
    username: &str,
    password: String,
) -> Result<SessionInfo, CmdError> {
    {
        let mut f = state.offline_failures.lock().unwrap();
        if let Some(until) = f.1 {
            if Instant::now() < until {
                let secs = (until - Instant::now()).as_secs().max(1) as i64;
                return Err(CmdError { retry_after_secs: Some(secs), ..CmdError::new("locked", format!("Too many failed attempts. Try again in {secs} seconds.")) });
            }
            *f = (0, None);
        }
    }
    let creds = state.cache.as_ref().and_then(|c| c.credentials(username).ok().flatten());
    let Some(creds) = creds.filter(|c| c.school.id == school_id) else {
        return Err(CmdError::new(
            "network",
            "Can't reach the Clinilog server. Offline login only works for an account that has logged in on this PC while online and hasn't logged out since.",
        ));
    };
    if !verify_password(password.clone(), creds.password_hash.clone()).await {
        let mut f = state.offline_failures.lock().unwrap();
        f.0 += 1;
        if f.0 >= OFFLINE_MAX_FAILURES {
            f.1 = Some(Instant::now() + OFFLINE_LOCK);
            return Err(CmdError {
                retry_after_secs: Some(OFFLINE_LOCK.as_secs() as i64),
                ..CmdError::new("locked", "Too many failed attempts. Try again in 300 seconds.")
            });
        }
        return Err(CmdError::new("bad_credentials", "Incorrect username or password."));
    }
    *state.offline_failures.lock().unwrap() = (0, None);
    let info = SessionInfo { user: creds.user.clone(), school: creds.school.clone(), offline: true };
    *state.session.lock().unwrap() = Some(Session {
        token: None,
        expires_at: None,
        user: creds.user,
        school: creds.school,
        password,
        auth_failed: false,
    });
    state.online.store(false, std::sync::atomic::Ordering::Relaxed);
    let _ = app; // sync loop picks the session up and reconnects when it can
    Ok(info)
}

/// Ends the session and clears the offline copy. Unsent changes stay (encrypted)
/// and upload the next time their author logs in online.
#[tauri::command]
fn logout(state: State<'_, AppState>) {
    state.clear_session();
    if let Some(c) = state.cache.as_ref() {
        let _ = c.clear_on_logout();
    }
}

/// Restores the session after a webview reload; `None` means show the login screen.
#[tauri::command]
fn current_session(state: State<'_, AppState>) -> Option<SessionInfo> {
    state.session.lock().unwrap().as_ref().map(|s| SessionInfo {
        user: s.user.clone(),
        school: s.school.clone(),
        offline: s.token.is_none(),
    })
}

/// The school's plan: from the server, or the last known plan while offline (7-day grace).
#[tauri::command]
async fn fetch_plan(state: State<'_, AppState>) -> Result<plan::PlanState, CmdError> {
    let (school_id, superadmin) = state
        .session
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| (s.school.id, s.is_superadmin()))
        .ok_or_else(CmdError::unauthorized)?;
    if superadmin {
        return Err(CmdError::new("invalid", "The superadmin account has no school plan."));
    }
    let data_dir = state.data_dir.as_deref();
    let fetched = match state.ensure_token().await {
        Ok(token) => state.api()?.get_authed::<SubscriptionInfo>(&token, "subscription").await,
        Err(e) => Err(e),
    };
    match fetched {
        Ok(info) => Ok(plan::save(data_dir, school_id, &info)),
        Err(e) if e.kind == "network" => plan::load_cached(data_dir, school_id).ok_or(e),
        Err(e) => {
            if e.kind == "unauthorized" {
                state.clear_session();
            }
            Err(e)
        }
    }
}

/// Generic authenticated call to the Clinilog API, e.g. `GET students?q=juan`.
/// Answered from the offline copy when the server can't be reached (see `sync.rs`).
#[tauri::command]
async fn api_request(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Result<Option<serde_json::Value>, CmdError> {
    let method = match method.to_ascii_uppercase().as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        _ => return Err(CmdError::new("invalid", "Unsupported request method.")),
    };
    let path = api::safe_api_path(&path)?;
    sync::request(&app, &state, method, path, body).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let paths = app.path();
            let config_dir = paths.app_config_dir().ok();
            let config = config::load(config_dir.as_deref());
            let api = config.as_ref().ok().map(|c| Api::new(c.api_url.clone()));
            let school = config.as_ref().ok().and_then(|c| c.school.clone());
            let data_dir = paths.app_data_dir().ok();
            let (cache, cache_error) = match data_dir.as_deref().map(cache::Cache::open) {
                Some(Ok(c)) => (Some(c), None),
                Some(Err(e)) => (None, Some(format!("Offline storage couldn't be opened: {e}"))),
                None => (None, Some("Offline storage is unavailable on this PC.".to_string())),
            };
            app.manage(AppState {
                config,
                school: Mutex::new(school),
                config_dir,
                api,
                data_dir,
                session: Mutex::new(None),
                cache,
                cache_error,
                online: AtomicBool::new(false),
                sync_wake: tokio::sync::Notify::new(),
                sync_lock: tokio::sync::Mutex::new(()),
                upload_lock: tokio::sync::Mutex::new(()),
                offline_failures: Mutex::new((0, None)),
            });
            tauri::async_runtime::spawn(sync::run(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_config,
            set_school,
            clear_school,
            check_server,
            login,
            logout,
            current_session,
            fetch_plan,
            api_request,
            pdf::save_report_pdf,
            sync::record_visit,
            sync::sync_status,
            sync::sync_now,
            sync::outbox_list,
            sync::outbox_retry,
            sync::outbox_discard
        ])
        .run(tauri::generate_context!())
        .expect("error while running Clinilog");
}
