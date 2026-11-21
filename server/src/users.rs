//! User accounts (admin only): list, add, edit name/role, disable/enable, reset password.
//!
//! Rules:
//! - Only active accounts count toward the plan limit (Basic 2, Premium 10).
//! - An admin can't disable or demote themselves, and the last active admin
//!   can't be disabled or demoted, so a school can't lock itself out.
//! - While the subscription is inactive, disabling and password resets still
//!   work (they protect access to records); adding, enabling and role changes don't.
//! - Every change is written to the activity log (table `users`).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    activity::{self, Action},
    auth::{password, AuthUser, Role},
    error::{unique_violation, AppError, AppResult},
    subscription::{current, ensure_writable},
    validate::{self, FieldErrors},
    AppState,
};

const TABLE: &str = "users";

#[derive(Serialize, sqlx::FromRow)]
pub struct UserListItem {
    pub id: Uuid,
    pub full_name: String,
    pub username: String,
    pub role: Role,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

macro_rules! select_user {
    () => {
        "SELECT id, full_name, username, role, is_active, created_at FROM users "
    };
}

/// `GET /users` — accounts in the admin's own school, active first.
pub async fn list(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Vec<UserListItem>>> {
    let rows = sqlx::query_as(concat!(select_user!(), "WHERE school_id = $1 ORDER BY is_active DESC, lower(full_name)"))
        .bind(user.school_id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(rows))
}

async fn fetch(conn: &mut PgConnection, school_id: Uuid, id: Uuid) -> AppResult<UserListItem> {
    sqlx::query_as(concat!(select_user!(), "WHERE id = $1 AND school_id = $2"))
        .bind(id)
        .bind(school_id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| AppError::NotFound("User not found.".into()))
}

/// Locks the school row so limit and last-admin checks can't race each other.
async fn lock_school(conn: &mut PgConnection, school_id: Uuid) -> AppResult<()> {
    sqlx::query("SELECT 1 FROM schools WHERE id = $1 FOR UPDATE")
        .bind(school_id)
        .execute(conn)
        .await?;
    Ok(())
}

async fn active_counts(conn: &mut PgConnection, school_id: Uuid) -> AppResult<(i64, i64)> {
    Ok(sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE role = 'admin')
           FROM users WHERE school_id = $1 AND is_active",
    )
    .bind(school_id)
    .fetch_one(conn)
    .await?)
}

/// Refuses when one more active account would exceed the plan's limit.
async fn check_account_limit(state: &AppState, conn: &mut PgConnection, school_id: Uuid) -> AppResult<()> {
    let limit = current(&state.db, school_id).await?.limits.max_users;
    let (active, _) = active_counts(conn, school_id).await?;
    if active >= limit {
        let plan_hint = if limit < 10 { " Upgrade to Premium for up to 10 accounts, or disable an account you no longer use." } else { " Disable an account you no longer use first." };
        return Err(AppError::Forbidden(format!("Your plan allows {limit} active user accounts.{plan_hint}")));
    }
    Ok(())
}

fn last_admin_error() -> AppError {
    AppError::Conflict {
        message: "This is the only active admin account. Make another account an admin first.".into(),
        code: "last_admin",
        details: None,
    }
}

async fn hash_password(pw: String) -> AppResult<String> {
    tokio::task::spawn_blocking(move || password::hash(&pw))
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .map_err(AppError::Internal)
}

fn parse_role(errors: &mut FieldErrors, value: &str) -> Role {
    match value.trim() {
        "admin" => Role::Admin,
        "staff" => Role::Staff,
        _ => {
            errors.add("role", "Choose a role.");
            Role::Staff
        }
    }
}

// ---------- Create ----------

#[derive(Deserialize)]
pub struct CreateInput {
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub role: String,
}

/// `POST /users`
pub async fn create(
    State(state): State<AppState>,
    admin: AuthUser,
    Json(input): Json<CreateInput>,
) -> AppResult<(StatusCode, Json<UserListItem>)> {
    ensure_writable(&state.db, admin.school_id).await?;

    let mut e = FieldErrors::default();
    let full_name = validate::full_name(&mut e, &input.full_name);
    let username = validate::username(&mut e, &input.username);
    validate::password(&mut e, "password", &input.password);
    let role = parse_role(&mut e, &input.role);

    let mut tx = state.db.begin().await?;
    lock_school(&mut tx, admin.school_id).await?;
    if !e.has("username") {
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM users WHERE school_id = $1 AND lower(username) = lower($2))",
        )
        .bind(admin.school_id)
        .bind(&username)
        .fetch_one(&mut *tx)
        .await?;
        if taken {
            e.add("username", "This username is already used in your school.");
        }
    }
    e.finish()?;
    check_account_limit(&state, &mut tx, admin.school_id).await?;

    let hash = hash_password(input.password).await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (school_id, full_name, username, password_hash, role) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(admin.school_id)
    .bind(&full_name)
    .bind(&username)
    .bind(&hash)
    .bind(role)
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| {
        if unique_violation(&err).is_some() {
            let mut f = FieldErrors::default();
            f.add("username", "This username is already used in your school.");
            return f.finish().unwrap_err();
        }
        err.into()
    })?;

    activity::log(&mut tx, &admin, Action::Create, TABLE, id).await?;
    let created = fetch(&mut tx, admin.school_id, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(created)))
}

// ---------- Edit name / role ----------

#[derive(Deserialize)]
pub struct UpdateInput {
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
    pub role: String,
}

/// `PUT /users/:id` — full name and role.
pub async fn update(
    State(state): State<AppState>,
    admin: AuthUser,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateInput>,
) -> AppResult<Json<UserListItem>> {
    ensure_writable(&state.db, admin.school_id).await?;
    let mut e = FieldErrors::default();
    let full_name = validate::full_name(&mut e, &input.full_name);
    let role = parse_role(&mut e, &input.role);
    e.finish()?;

    let mut tx = state.db.begin().await?;
    lock_school(&mut tx, admin.school_id).await?;
    let target = fetch(&mut tx, admin.school_id, id).await?;

    if target.role == Role::Admin && role == Role::Staff {
        if target.id == admin.id {
            return Err(AppError::Forbidden("You can't remove your own admin role.".into()));
        }
        let (_, admins) = active_counts(&mut tx, admin.school_id).await?;
        if target.is_active && admins <= 1 {
            return Err(last_admin_error());
        }
    }

    sqlx::query("UPDATE users SET full_name = $3, role = $4 WHERE id = $1 AND school_id = $2")
        .bind(id)
        .bind(admin.school_id)
        .bind(&full_name)
        .bind(role)
        .execute(&mut *tx)
        .await?;
    activity::log(&mut tx, &admin, Action::Update, TABLE, id).await?;
    let updated = fetch(&mut tx, admin.school_id, id).await?;
    tx.commit().await?;
    Ok(Json(updated))
}

// ---------- Disable / enable ----------

/// `POST /users/:id/disable` — takes effect on the account's next request.
pub async fn disable(State(state): State<AppState>, admin: AuthUser, Path(id): Path<Uuid>) -> AppResult<Json<UserListItem>> {
    let mut tx = state.db.begin().await?;
    lock_school(&mut tx, admin.school_id).await?;
    let target = fetch(&mut tx, admin.school_id, id).await?;
    if target.id == admin.id {
        return Err(AppError::Forbidden("You can't disable your own account.".into()));
    }
    if !target.is_active {
        return Ok(Json(target));
    }
    if target.role == Role::Admin {
        let (_, admins) = active_counts(&mut tx, admin.school_id).await?;
        if admins <= 1 {
            return Err(last_admin_error());
        }
    }
    sqlx::query("UPDATE users SET is_active = false WHERE id = $1 AND school_id = $2")
        .bind(id)
        .bind(admin.school_id)
        .execute(&mut *tx)
        .await?;
    activity::log(&mut tx, &admin, Action::Update, TABLE, id).await?;
    let updated = fetch(&mut tx, admin.school_id, id).await?;
    tx.commit().await?;
    Ok(Json(updated))
}

/// `POST /users/:id/enable` — counts toward the plan limit again.
pub async fn enable(State(state): State<AppState>, admin: AuthUser, Path(id): Path<Uuid>) -> AppResult<Json<UserListItem>> {
    ensure_writable(&state.db, admin.school_id).await?;
    let mut tx = state.db.begin().await?;
    lock_school(&mut tx, admin.school_id).await?;
    let target = fetch(&mut tx, admin.school_id, id).await?;
    if target.is_active {
        return Ok(Json(target));
    }
    check_account_limit(&state, &mut tx, admin.school_id).await?;
    sqlx::query("UPDATE users SET is_active = true WHERE id = $1 AND school_id = $2")
        .bind(id)
        .bind(admin.school_id)
        .execute(&mut *tx)
        .await?;
    activity::log(&mut tx, &admin, Action::Update, TABLE, id).await?;
    let updated = fetch(&mut tx, admin.school_id, id).await?;
    tx.commit().await?;
    Ok(Json(updated))
}

// ---------- Reset password ----------

#[derive(Deserialize)]
pub struct ResetInput {
    #[serde(default)]
    pub password: String,
}

/// `POST /users/:id/reset-password` — sets a new password and clears any login lock.
pub async fn reset_password(
    State(state): State<AppState>,
    admin: AuthUser,
    Path(id): Path<Uuid>,
    Json(input): Json<ResetInput>,
) -> AppResult<StatusCode> {
    let mut e = FieldErrors::default();
    validate::password(&mut e, "password", &input.password);
    e.finish()?;

    let mut tx = state.db.begin().await?;
    let target = fetch(&mut tx, admin.school_id, id).await?;
    let hash = hash_password(input.password).await?;
    sqlx::query("UPDATE users SET password_hash = $3 WHERE id = $1 AND school_id = $2")
        .bind(id)
        .bind(admin.school_id)
        .bind(&hash)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM login_attempts WHERE school_id = $1 AND username = lower($2)")
        .bind(admin.school_id)
        .bind(&target.username)
        .execute(&mut *tx)
        .await?;
    activity::log(&mut tx, &admin, Action::Update, TABLE, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
