//! Schools (tenants). The superadmin creates each school with its first admin; the
//! school starts on a free 30-day Basic trial. Staff connect a copy of the desktop
//! app to their school by typing its school code once.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::{password, SuperadminAuth},
    error::{unique_violation, AppError, AppResult},
    subscription::Plan,
    validate::{self, FieldErrors},
    AppState,
};

/// Length of a new school's free trial, counting the first day.
pub const TRIAL_DAYS: i64 = 30;

/// "vnhs " → "VNHS". Codes are upper-case letters, digits and hyphens.
pub fn normalize_code(input: &str) -> String {
    input.trim().to_uppercase()
}

fn valid_code(code: &str) -> bool {
    (3..=20).contains(&code.len()) && code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
}

// ---------- Public: find a school by code ----------

#[derive(Deserialize)]
pub struct LookupQuery {
    #[serde(default)]
    pub code: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct SchoolRef {
    pub id: Uuid,
    pub name: String,
    pub code: String,
}

/// `GET /schools/lookup?code=VNHS` — no login needed. The app calls it once, when
/// it's set up for a school, and remembers the answer.
pub async fn lookup(State(state): State<AppState>, Query(q): Query<LookupQuery>) -> AppResult<Json<SchoolRef>> {
    let code = normalize_code(&q.code);
    if !valid_code(&code) {
        return Err(AppError::NotFound("No school has that code. Check it with your Clinilog provider.".into()));
    }
    sqlx::query_as("SELECT id, name, code FROM schools WHERE code = $1")
        .bind(&code)
        .fetch_optional(&state.db)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::NotFound("No school has that code. Check it with your Clinilog provider.".into()))
}

// ---------- Superadmin: create and list schools ----------

#[derive(Deserialize)]
pub struct CreateInput {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub address: String,
    /// The school's first admin.
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Serialize)]
pub struct Created {
    pub school: SchoolRef,
    pub admin_username: String,
    pub admin_full_name: String,
    pub trial_start: NaiveDate,
    /// Last day of the trial (inclusive).
    pub trial_end: NaiveDate,
}

/// `POST /superadmin/schools` — a new school, its first admin, the usual medicines,
/// and a 30-day Basic trial starting today.
pub async fn create(
    State(state): State<AppState>,
    _sa: SuperadminAuth,
    Json(input): Json<CreateInput>,
) -> AppResult<(StatusCode, Json<Created>)> {
    let mut e = FieldErrors::default();
    let name = input.name.trim().to_string();
    match name.chars().count() {
        0 => e.add("name", "Enter the school's name."),
        n if n > 200 => e.add("name", "School name must be 200 characters or fewer."),
        _ => {}
    }
    let code = normalize_code(&input.code);
    if code.is_empty() {
        e.add("code", "Enter a school code, e.g. VNHS.");
    } else if !valid_code(&code) {
        e.add("code", "School code must be 3–20 letters, numbers or hyphens (-), e.g. VNHS.");
    }
    let address = input.address.trim().to_string();
    if address.chars().count() > 300 {
        e.add("address", "Address must be 300 characters or fewer.");
    }
    let full_name = validate::full_name(&mut e, &input.full_name);
    let username = validate::username(&mut e, &input.username);
    if state.superadmin.as_deref().is_some_and(|sa| sa.username == username.to_lowercase()) {
        e.add("username", "This username is reserved. Choose another.");
    }
    validate::password(&mut e, "password", &input.password);
    e.finish()?;

    let pw = input.password;
    let hash = tokio::task::spawn_blocking(move || password::hash(&pw))
        .await
        .map_err(|e| AppError::Internal(e.into()))??;

    let today = validate::today_ph();
    let trial_end = today + chrono::Duration::days(TRIAL_DAYS - 1);

    let mut tx = state.db.begin().await?;
    let school_id: Uuid = sqlx::query_scalar("INSERT INTO schools (name, address, code) VALUES ($1, $2, $3) RETURNING id")
        .bind(&name)
        .bind(&address)
        .bind(&code)
        .fetch_one(&mut *tx)
        .await
        .map_err(|err| match unique_violation(&err).as_deref() {
            Some("schools_code_key") => {
                let mut f = FieldErrors::default();
                f.add("code", "Another school already uses this code.");
                f.finish().unwrap_err()
            }
            _ => err.into(),
        })?;
    sqlx::query(
        "INSERT INTO subscriptions (school_id, plan, start_date, end_date, status, is_trial)
         VALUES ($1, 'basic', $2, $3, 'active', true)",
    )
    .bind(school_id)
    .bind(today)
    .bind(trial_end)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO users (school_id, full_name, username, password_hash, role) VALUES ($1, $2, $3, $4, 'admin')")
        .bind(school_id)
        .bind(&full_name)
        .bind(&username)
        .bind(&hash)
        .execute(&mut *tx)
        .await?;
    for (med, unit) in crate::seed::SAMPLE_MEDICINES {
        sqlx::query("INSERT INTO medicines (school_id, name, unit) VALUES ($1, $2, $3)")
            .bind(school_id)
            .bind(med)
            .bind(unit)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    tracing::info!(%school_id, %code, "school created by superadmin");

    Ok((
        StatusCode::CREATED,
        Json(Created {
            school: SchoolRef { id: school_id, name, code },
            admin_username: username,
            admin_full_name: full_name,
            trial_start: today,
            trial_end,
        }),
    ))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct SchoolSummary {
    pub id: Uuid,
    pub name: String,
    pub code: String,
    pub created_at: DateTime<Utc>,
    pub plan: Option<Plan>,
    /// `active`, `expired`, `cancelled`, or null when the school has no plan.
    pub status: Option<String>,
    pub is_trial: Option<bool>,
    pub end_date: Option<NaiveDate>,
    pub active_users: i64,
    pub students: i64,
    pub pending_requests: i64,
    pub last_paid_at: Option<DateTime<Utc>>,
}

/// `GET /superadmin/schools` — every school with its current plan, newest first.
pub async fn list(State(state): State<AppState>, _sa: SuperadminAuth) -> AppResult<Json<Vec<SchoolSummary>>> {
    let today = validate::today_ph();
    let rows = sqlx::query_as(
        "SELECT sc.id, sc.name, sc.code, sc.created_at, s.plan,
                CASE WHEN s.status = 'active' AND s.end_date < $1 THEN 'expired' ELSE s.status END AS status,
                s.is_trial, s.end_date,
                (SELECT count(*) FROM users u WHERE u.school_id = sc.id AND u.is_active) AS active_users,
                (SELECT count(*) FROM students st WHERE st.school_id = sc.id) AS students,
                (SELECT count(*) FROM subscription_requests r WHERE r.school_id = sc.id AND r.status = 'pending') AS pending_requests,
                (SELECT max(r.reviewed_at) FROM subscription_requests r WHERE r.school_id = sc.id AND r.status = 'approved') AS last_paid_at
           FROM schools sc
           LEFT JOIN LATERAL (
                SELECT s1.plan, s1.status,
                       -- a trial already followed by a paid period counts as paid
                       s1.is_trial AND NOT EXISTS (
                           SELECT 1 FROM subscriptions s3
                            WHERE s3.school_id = sc.id AND s3.status = 'active' AND s3.plan = s1.plan
                              AND s3.start_date > s1.start_date AND s3.end_date > s1.end_date) AS is_trial,
                       -- valid until the end of any renewals already paid for
                       greatest(s1.end_date,
                                (SELECT max(s2.end_date) FROM subscriptions s2
                                  WHERE s2.school_id = sc.id AND s2.status = 'active'
                                    AND s2.plan = s1.plan AND s2.start_date > s1.start_date)) AS end_date
                  FROM subscriptions s1
                 WHERE s1.school_id = sc.id
                 ORDER BY (s1.status = 'active') DESC, (s1.start_date <= $1) DESC, s1.end_date DESC
                 LIMIT 1) s ON true
          ORDER BY sc.created_at DESC",
    )
    .bind(today)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(normalize_code(" vnhs "), "VNHS");
        assert!(valid_code("VNHS"));
        assert!(valid_code("SAN-ISIDRO-NHS"));
        assert!(!valid_code("AB"));
        assert!(!valid_code("VN HS"));
        assert!(!valid_code("VNHS!"));
    }
}
