use axum::{
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// API error. Every variant becomes a JSON body `{ "error": "<message>" }`.
/// Internal errors are logged in full but shown to the client only as a generic message.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    /// One message per invalid field: `{ "error": "...", "fields": { "lrn": "..." } }`.
    #[error("Please fix the highlighted fields.")]
    Validation(std::collections::BTreeMap<String, String>),
    /// 409. `code` lets the app react (e.g. `possible_duplicate`), `details` carries extra data.
    #[error("{message}")]
    Conflict {
        message: String,
        code: &'static str,
        details: Option<serde_json::Value>,
    },
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    /// 403 with `code: "premium_required"` so the app can show the upgrade message.
    #[error("{0}")]
    PremiumRequired(String),
    /// 403 with `code: "read_only"`: the subscription is inactive. Changes saved offline
    /// wait and upload once it's renewed, instead of being treated as refused.
    #[error("{0}")]
    ReadOnly(String),
    #[error("Too many failed login attempts. Try again in {retry_after_secs} seconds.")]
    LoginLocked { retry_after_secs: i64 },
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Internal(e.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::BadRequest(_) | AppError::Validation(_) => StatusCode::BAD_REQUEST,
            AppError::Conflict { .. } => StatusCode::CONFLICT,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) | AppError::PremiumRequired(_) | AppError::ReadOnly(_) => StatusCode::FORBIDDEN,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::LoginLocked { .. } => StatusCode::TOO_MANY_REQUESTS,
            AppError::Internal(e) => {
                tracing::error!(error = ?e, "internal error");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        let message = match &self {
            AppError::Internal(_) => "Something went wrong on the server.".to_string(),
            other => other.to_string(),
        };

        let mut body = json!({ "error": message });
        match &self {
            AppError::LoginLocked { retry_after_secs } => body["retry_after_secs"] = json!(retry_after_secs),
            AppError::Validation(fields) => body["fields"] = json!(fields),
            AppError::PremiumRequired(_) => body["code"] = json!("premium_required"),
            AppError::ReadOnly(_) => body["code"] = json!("read_only"),
            AppError::Conflict { code, details, .. } => {
                body["code"] = json!(code);
                if let Some(d) = details {
                    body["details"] = d.clone();
                }
            }
            _ => {}
        }
        let mut res = (status, Json(body)).into_response();

        match &self {
            AppError::Unauthorized(_) => {
                res.headers_mut()
                    .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
            }
            AppError::LoginLocked { retry_after_secs } => {
                if let Ok(v) = HeaderValue::from_str(&retry_after_secs.to_string()) {
                    res.headers_mut().insert(header::RETRY_AFTER, v);
                }
            }
            _ => {}
        }
        res
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// The constraint name if `e` is a Postgres unique violation (SQLSTATE 23505).
pub fn unique_violation(e: &sqlx::Error) -> Option<String> {
    match e {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => db.constraint().map(str::to_string),
        _ => None,
    }
}
