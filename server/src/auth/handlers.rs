use axum::{extract::State, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{middleware::Role, password, AuthUser};
use crate::{
    error::{AppError, AppResult},
    AppState,
};

/// Failed attempts allowed before the username is locked.
const MAX_FAILED_ATTEMPTS: i32 = 5;
/// How long a lock lasts.
const LOCK_MINUTES: i32 = 5;
/// Failures older than this no longer count toward a lock.
const FAILURE_WINDOW_MINUTES: i32 = 15;

const BAD_CREDENTIALS: &str = "Incorrect username or password.";

#[derive(Deserialize)]
pub struct LoginRequest {
    /// Comes from the desktop app's config, not typed by staff.
    pub school_id: Uuid,
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub token_type: &'static str,
    pub expires_at: DateTime<Utc>,
    pub user: UserInfo,
    pub school: SchoolInfo,
}

#[derive(Serialize)]
pub struct UserInfo {
    pub id: Uuid,
    pub full_name: String,
    pub username: String,
    pub role: Role,
}

#[derive(Serialize)]
pub struct SchoolInfo {
    pub id: Uuid,
    pub name: String,
}

#[derive(sqlx::FromRow)]
struct LoginRow {
    id: Uuid,
    full_name: String,
    username: String,
    role: Role,
    password_hash: String,
    is_active: bool,
}

/// `POST /auth/login`
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    // Usernames are case-insensitive; passwords are compared exactly (not trimmed).
    let username = req.username.trim().to_lowercase();
    if username.is_empty() || req.password.is_empty() {
        return Err(AppError::BadRequest("Enter your username and password.".into()));
    }
    // No account can have a username outside these rules, so there's nothing to check or lock.
    let well_formed = (4..=30).contains(&username.len())
        && username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !well_formed || req.password.len() > 1024 {
        return Err(AppError::Unauthorized(BAD_CREDENTIALS.into()));
    }

    let school_name: Option<String> = sqlx::query_scalar("SELECT name FROM schools WHERE id = $1")
        .bind(req.school_id)
        .fetch_optional(&state.db)
        .await?;
    let Some(school_name) = school_name else {
        return Err(AppError::Unauthorized(BAD_CREDENTIALS.into()));
    };

    if let Some(secs) = lock_remaining_secs(&state, req.school_id, &username).await? {
        return Err(AppError::LoginLocked { retry_after_secs: secs });
    }

    // The superadmin signs in from any school's app with the username set on the server.
    if let Some(sa) = state.superadmin.clone().filter(|sa| sa.username == username) {
        return superadmin_login(&state, req.school_id, &username, req.password, sa).await;
    }

    let row: Option<LoginRow> = sqlx::query_as(
        "SELECT id, full_name, username, role, password_hash, is_active
           FROM users
          WHERE school_id = $1 AND lower(username) = $2",
    )
    .bind(req.school_id)
    .bind(&username)
    .fetch_optional(&state.db)
    .await?;

    // Always run a full Argon2 verify so timing doesn't reveal whether the user exists.
    let stored_hash = row
        .as_ref()
        .map(|r| r.password_hash.clone())
        .unwrap_or_else(|| password::dummy_hash().to_string());
    let password_input = req.password;
    let password_ok = tokio::task::spawn_blocking(move || password::verify(&password_input, &stored_hash))
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let user = match row {
        Some(r) if password_ok => r,
        _ => {
            return Err(record_failure(&state, req.school_id, &username).await?);
        }
    };

    if !user.is_active {
        return Err(AppError::Forbidden(
            "This account has been disabled. Ask your clinic admin.".into(),
        ));
    }

    sqlx::query("DELETE FROM login_attempts WHERE school_id = $1 AND username = $2")
        .bind(req.school_id)
        .bind(&username)
        .execute(&state.db)
        .await?;

    let role_str = match user.role {
        Role::Admin => "admin",
        Role::Staff => "staff",
        Role::Superadmin => return Err(AppError::Unauthorized(BAD_CREDENTIALS.into())),
    };
    let (token, expires_at) = state.jwt.issue(user.id, req.school_id, role_str)?;

    tracing::info!(user_id = %user.id, school_id = %req.school_id, "login succeeded");

    Ok(Json(LoginResponse {
        token,
        token_type: "Bearer",
        expires_at,
        user: UserInfo {
            id: user.id,
            full_name: user.full_name,
            username: user.username,
            role: user.role,
        },
        school: SchoolInfo { id: req.school_id, name: school_name },
    }))
}

async fn superadmin_login(
    state: &AppState,
    school_id: Uuid,
    username: &str,
    password: String,
    sa: std::sync::Arc<super::superadmin::Superadmin>,
) -> AppResult<Json<LoginResponse>> {
    let checker = sa.clone();
    let ok = tokio::task::spawn_blocking(move || checker.check_password(&password))
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    if !ok {
        return Err(record_failure(state, school_id, username).await?);
    }
    sqlx::query("DELETE FROM login_attempts WHERE school_id = $1 AND username = $2")
        .bind(school_id)
        .bind(username)
        .execute(&state.db)
        .await?;
    // No school: the token can't reach any school's records (see `require_auth`).
    let (token, expires_at) = state.jwt.issue(sa.session_id, Uuid::nil(), super::superadmin::ROLE)?;
    tracing::info!(%school_id, "superadmin login succeeded");
    Ok(Json(LoginResponse {
        token,
        token_type: "Bearer",
        expires_at,
        user: UserInfo {
            id: sa.session_id,
            full_name: super::superadmin::FULL_NAME.into(),
            username: sa.username.clone(),
            role: Role::Superadmin,
        },
        // The app's school id, so the app can sign back in by itself when the token expires.
        school: SchoolInfo { id: school_id, name: "Clinilog".into() },
    }))
}

/// Seconds left on an active lock, if any.
async fn lock_remaining_secs(state: &AppState, school_id: Uuid, username: &str) -> AppResult<Option<i64>> {
    let secs: Option<i64> = sqlx::query_scalar(
        "SELECT CEIL(EXTRACT(EPOCH FROM (locked_until - now())))::bigint
           FROM login_attempts
          WHERE school_id = $1 AND username = $2 AND locked_until > now()",
    )
    .bind(school_id)
    .bind(username)
    .fetch_optional(&state.db)
    .await?;
    Ok(secs.map(|s| s.max(1)))
}

/// Counts a failed attempt and locks the username once the limit is reached.
/// Returns the error to send back: a lock message if this attempt triggered one.
async fn record_failure(state: &AppState, school_id: Uuid, username: &str) -> AppResult<AppError> {
    let mut tx = state.db.begin().await?;

    let count: i32 = sqlx::query_scalar(
        "INSERT INTO login_attempts (school_id, username, failed_count, last_failed_at)
         VALUES ($1, $2, 1, now())
         ON CONFLICT (school_id, username) DO UPDATE SET
             failed_count = CASE
                 WHEN login_attempts.last_failed_at < now() - make_interval(mins => $3)
                 THEN 1
                 ELSE login_attempts.failed_count + 1
             END,
             last_failed_at = now()
         RETURNING failed_count",
    )
    .bind(school_id)
    .bind(username)
    .bind(FAILURE_WINDOW_MINUTES)
    .fetch_one(&mut *tx)
    .await?;

    let err = if count >= MAX_FAILED_ATTEMPTS {
        sqlx::query(
            "UPDATE login_attempts
                SET failed_count = 0, locked_until = now() + make_interval(mins => $3)
              WHERE school_id = $1 AND username = $2",
        )
        .bind(school_id)
        .bind(username)
        .bind(LOCK_MINUTES)
        .execute(&mut *tx)
        .await?;
        tracing::warn!(%school_id, username, "login locked after repeated failures");
        AppError::LoginLocked { retry_after_secs: i64::from(LOCK_MINUTES) * 60 }
    } else {
        AppError::Unauthorized(BAD_CREDENTIALS.into())
    };

    tx.commit().await?;
    Ok(err)
}

#[derive(Serialize)]
pub struct MeResponse {
    pub user: UserInfo,
    pub school: SchoolInfo,
}

/// `GET /auth/me` — who is logged in. Lets the app restore a session on launch.
pub async fn me(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<MeResponse>> {
    let school_name: String = sqlx::query_scalar("SELECT name FROM schools WHERE id = $1")
        .bind(user.school_id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(MeResponse {
        school: SchoolInfo { id: user.school_id, name: school_name },
        user: UserInfo {
            id: user.id,
            full_name: user.full_name,
            username: user.username,
            role: user.role,
        },
    }))
}
