use axum::{
    extract::{FromRequestParts, Request, State},
    http::{header, request::Parts},
    middleware::Next,
    response::Response,
};
use serde::Serialize;
use uuid::Uuid;

use crate::{error::AppError, AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum Role {
    Admin,
    Staff,
}

/// The logged-in user, loaded fresh from the database on every request.
///
/// `school_id` is the tenant boundary: every query in a handler must filter by
/// `user.school_id`. It never comes from the request body or URL.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AuthUser {
    pub id: Uuid,
    pub school_id: Uuid,
    pub role: Role,
    pub full_name: String,
    pub username: String,
}

impl AuthUser {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }
}

/// Middleware: requires a valid `Authorization: Bearer <token>` for an active user.
/// Puts the [`AuthUser`] into request extensions for handlers and later middleware.
pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| AppError::Unauthorized("Please log in.".into()))?;

    let claims = state
        .jwt
        .verify(token)
        .ok_or_else(|| AppError::Unauthorized("Your session has expired. Please log in again.".into()))?;

    // Re-check the account: a disabled user or a role change takes effect immediately,
    // not when the token expires.
    let user: Option<AuthUser> = sqlx::query_as(
        "SELECT id, school_id, role, full_name, username
           FROM users
          WHERE id = $1 AND school_id = $2 AND is_active",
    )
    .bind(claims.sub)
    .bind(claims.school_id)
    .fetch_optional(&state.db)
    .await?;

    let user = user.ok_or_else(|| {
        AppError::Unauthorized("Your account is no longer active. Please log in again.".into())
    })?;

    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

/// Middleware: admin-only. Must be layered inside [`require_auth`].
pub async fn require_admin(req: Request, next: Next) -> Result<Response, AppError> {
    match req.extensions().get::<AuthUser>() {
        Some(u) if u.is_admin() => Ok(next.run(req).await),
        Some(_) => Err(AppError::Forbidden("Only an admin can do this.".into())),
        None => Err(AppError::Internal(anyhow::anyhow!(
            "require_admin used without require_auth"
        ))),
    }
}

/// Lets handlers take `user: AuthUser` directly. Fails loudly (500) if a route
/// that needs it was mounted outside the `require_auth` layer.
impl<S: Send + Sync> FromRequestParts<S> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<AuthUser>().cloned().ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("AuthUser extractor used without require_auth"))
        })
    }
}
