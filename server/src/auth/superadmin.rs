//! The Clinilog superadmin: one account, set with `SUPERADMIN_USERNAME` and
//! `SUPERADMIN_PASSWORD` on the server (Render → Environment). It is not a school
//! user and can't see clinic records; it only reviews subscription payments.

use anyhow::{anyhow, bail, Result};
use argon2::Argon2;
use axum::{
    extract::{FromRequestParts, Request, State},
    http::{header, request::Parts},
    middleware::Next,
    response::Response,
};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use uuid::Uuid;

use crate::{error::AppError, AppState};

/// Same lockout rule as school accounts: 5 wrong passwords within 15 minutes lock it for 5.
const MAX_FAILURES: u32 = 5;
const FAILURE_WINDOW: Duration = Duration::from_secs(15 * 60);
const LOCK_FOR: Duration = Duration::from_secs(5 * 60);

/// Failed attempts, kept in memory: the superadmin isn't tied to any school's rows.
#[derive(Default)]
struct Attempts {
    count: u32,
    last_failed: Option<Instant>,
    locked_until: Option<Instant>,
}

pub const ROLE: &str = "superadmin";
pub const FULL_NAME: &str = "Clinilog Superadmin";

const VERIFIER_SALT: &[u8] = b"clinilog-superadmin-verifier-v1";
const SESSION_SALT: &[u8] = b"clinilog-superadmin-session-v1";

pub struct Superadmin {
    /// Lower-case; logins match it case-insensitively.
    pub username: String,
    /// Argon2 of the password with a fixed salt. The password itself isn't kept.
    verifier: [u8; 32],
    /// Goes in the token as `sub`. Derived from the username and password, so it stays
    /// the same across restarts and changing the password signs every session out.
    pub session_id: Uuid,
    attempts: Mutex<Attempts>,
}

fn derive(input: &[u8], salt: &[u8], out: &mut [u8]) -> Result<()> {
    Argon2::default()
        .hash_password_into(input, salt, out)
        .map_err(|e| anyhow!("argon2: {e}"))
}

impl Superadmin {
    /// `None` when `SUPERADMIN_PASSWORD` is not set: the superadmin login is then off.
    pub fn from_env() -> Result<Option<Self>> {
        let Some(password) = std::env::var("SUPERADMIN_PASSWORD").ok().filter(|p| !p.is_empty()) else {
            return Ok(None);
        };
        let username = std::env::var("SUPERADMIN_USERNAME").unwrap_or_else(|_| "superadmin".into());
        Self::new(&username, &password).map(Some)
    }

    pub fn new(username: &str, password: &str) -> Result<Self> {
        let username = username.trim().to_lowercase();
        if !(4..=30).contains(&username.len()) || !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            bail!("SUPERADMIN_USERNAME must be 4–30 letters, numbers or underscore");
        }
        let strong = password.chars().count() >= 12
            && password.len() <= 128
            && password.chars().any(char::is_alphabetic)
            && password.chars().any(|c| c.is_ascii_digit());
        if !strong {
            bail!("SUPERADMIN_PASSWORD must be 12–128 characters with at least one letter and one number");
        }
        let mut verifier = [0u8; 32];
        derive(password.as_bytes(), VERIFIER_SALT, &mut verifier)?;
        let mut sid = [0u8; 16];
        derive(format!("{username}\n{password}").as_bytes(), SESSION_SALT, &mut sid)?;
        Ok(Self { username, verifier, session_id: Uuid::from_bytes(sid), attempts: Mutex::default() })
    }

    /// Seconds left on a lock, if locked.
    pub fn locked_secs(&self) -> Option<i64> {
        let a = self.attempts.lock().unwrap();
        a.locked_until
            .and_then(|t| t.checked_duration_since(Instant::now()))
            .map(|d| d.as_secs().max(1) as i64)
    }

    /// Counts a wrong password. Returns the lock length in seconds if this one locked it.
    pub fn record_failure(&self) -> Option<i64> {
        let mut a = self.attempts.lock().unwrap();
        let now = Instant::now();
        if a.last_failed.is_none_or(|t| now.duration_since(t) > FAILURE_WINDOW) {
            a.count = 0;
        }
        a.count += 1;
        a.last_failed = Some(now);
        if a.count >= MAX_FAILURES {
            a.count = 0;
            a.locked_until = Some(now + LOCK_FOR);
            tracing::warn!("superadmin login locked after repeated failures");
            return Some(LOCK_FOR.as_secs() as i64);
        }
        None
    }

    pub fn clear_failures(&self) {
        *self.attempts.lock().unwrap() = Attempts::default();
    }

    /// Case-sensitive password check. Slow on purpose (Argon2), like a normal login.
    pub fn check_password(&self, input: &str) -> bool {
        if input.len() > 1024 {
            return false;
        }
        let mut got = [0u8; 32];
        if derive(input.as_bytes(), VERIFIER_SALT, &mut got).is_err() {
            return false;
        }
        // Constant-time compare.
        got.iter().zip(self.verifier.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
    }
}

/// Marker for handlers mounted behind [`require_superadmin`].
#[derive(Clone, Copy)]
pub struct SuperadminAuth;

/// Middleware: a valid superadmin token, issued with the current superadmin password.
pub async fn require_superadmin(State(state): State<AppState>, mut req: Request, next: Next) -> Result<Response, AppError> {
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
    if claims.role != ROLE {
        return Err(AppError::Forbidden("Only the Clinilog superadmin can do this.".into()));
    }
    match state.superadmin.as_deref() {
        Some(sa) if sa.session_id == claims.sub => {}
        _ => return Err(AppError::Unauthorized("The superadmin password changed. Please log in again.".into())),
    }
    req.extensions_mut().insert(SuperadminAuth);
    Ok(next.run(req).await)
}

impl<S: Send + Sync> FromRequestParts<S> for SuperadminAuth {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<SuperadminAuth>().copied().ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("SuperadminAuth used without require_superadmin"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_password_exactly() {
        let sa = Superadmin::new("SuperAdmin", "Clinilog2026!x").unwrap();
        assert_eq!(sa.username, "superadmin");
        assert!(sa.check_password("Clinilog2026!x"));
        assert!(!sa.check_password("clinilog2026!x"));
        assert!(!sa.check_password(""));
    }

    #[test]
    fn session_id_is_stable_and_changes_with_password() {
        let a = Superadmin::new("superadmin", "Clinilog2026!x").unwrap();
        let b = Superadmin::new("superadmin", "Clinilog2026!x").unwrap();
        let c = Superadmin::new("superadmin", "Clinilog2026!y").unwrap();
        assert_eq!(a.session_id, b.session_id);
        assert_ne!(a.session_id, c.session_id);
    }

    #[test]
    fn locks_after_five_failures() {
        let sa = Superadmin::new("superadmin", "Clinilog2026!x").unwrap();
        for _ in 0..4 {
            assert!(sa.record_failure().is_none());
        }
        assert_eq!(sa.record_failure(), Some(300));
        assert!(sa.locked_secs().is_some());
        sa.clear_failures();
        assert!(sa.locked_secs().is_none());
    }

    #[test]
    fn rejects_weak_settings() {
        assert!(Superadmin::new("superadmin", "short1").is_err());
        assert!(Superadmin::new("superadmin", "onlyletterslong").is_err());
        assert!(Superadmin::new("bad name", "Clinilog2026!x").is_err());
    }
}
