//! `GET /sync` — the school's clinic records for the desktop app's offline copy.
//!
//! Students, visits and deletions come in pages ordered by (time, id). The app
//! keeps the position of the last row it got for each kind (a cursor) and asks
//! only for what changed after it. Sections, medicines, staff names and accounts
//! are small and come in full every time.
//!
//! `accounts` lets every active account of the school log in on a PC that is
//! offline, even one it never used before. It holds each account's Argon2 hash
//! (never a password), which the app keeps encrypted. The superadmin isn't a
//! school account, so it is never included.
//!
//! Cursors are `<rfc3339 time>|<uuid>`. A row's time is set when its transaction
//! starts, so a slow transaction can commit a row "behind" a cursor; the app
//! starts each sync a few minutes before its last cursor to pick those up
//! (re-sent rows are simply applied again).

use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json as SqlJson;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{AppError, AppResult},
    AppState,
};

const DEFAULT_LIMIT: i64 = 500;
const MAX_LIMIT: i64 = 1000;

#[derive(Deserialize)]
pub struct SyncQuery {
    pub students: Option<String>,
    pub visits: Option<String>,
    pub deleted: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Clone, Copy)]
struct Cursor {
    at: DateTime<Utc>,
    id: Uuid,
}

impl Cursor {
    fn parse(value: Option<&str>) -> AppResult<Option<Self>> {
        let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else { return Ok(None) };
        let bad = || AppError::BadRequest("Invalid sync position.".into());
        let (at, id) = v.split_once('|').ok_or_else(bad)?;
        Ok(Some(Cursor {
            at: DateTime::parse_from_rfc3339(at).map_err(|_| bad())?.with_timezone(&Utc),
            id: id.parse().map_err(|_| bad())?,
        }))
    }

    fn format(at: DateTime<Utc>, id: Uuid) -> String {
        format!("{}|{id}", at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
    }
}

#[derive(Serialize, sqlx::FromRow)]
pub struct SyncSection {
    pub id: Uuid,
    pub grade_level: i32,
    pub section_name: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct SyncMedicine {
    pub id: Uuid,
    pub name: String,
    pub unit: String,
}

/// Everyone who can appear as "attended by" on a visit, including disabled accounts.
#[derive(Serialize, sqlx::FromRow)]
pub struct Staff {
    pub id: Uuid,
    pub full_name: String,
}

/// An active account of the school, for offline login.
#[derive(Serialize, sqlx::FromRow)]
pub struct SyncAccount {
    pub id: Uuid,
    pub full_name: String,
    pub username: String,
    pub role: String,
    pub password_hash: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct SyncSchool {
    pub id: Uuid,
    pub name: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct SyncStudent {
    pub id: Uuid,
    pub lrn: String,
    pub first_name: String,
    pub middle_name: Option<String>,
    pub last_name: String,
    pub gender: String,
    pub birthdate: NaiveDate,
    pub section_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
pub struct SyncLine {
    pub medicine_id: Uuid,
    pub quantity: i32,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct SyncVisit {
    pub id: Uuid,
    pub student_id: Uuid,
    pub user_id: Uuid,
    pub visited_at: DateTime<Utc>,
    pub complaint: String,
    pub treatment: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub medicines: SqlJson<Vec<SyncLine>>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Deletion {
    /// `students` or `clinic_visits`.
    pub table_name: String,
    pub record_id: Uuid,
    pub deleted_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct Cursors {
    pub students: Option<String>,
    pub visits: Option<String>,
    pub deleted: Option<String>,
}

#[derive(Serialize)]
pub struct SyncResponse {
    pub school: SyncSchool,
    pub accounts: Vec<SyncAccount>,
    pub sections: Vec<SyncSection>,
    pub medicines: Vec<SyncMedicine>,
    pub staff: Vec<Staff>,
    pub students: Vec<SyncStudent>,
    pub visits: Vec<SyncVisit>,
    pub deleted: Vec<Deletion>,
    /// Where to continue from (unchanged for a kind with no new rows).
    pub next: Cursors,
    /// A page was full for at least one kind: call again with `next`.
    pub more: bool,
}

/// `GET /sync?students=&visits=&deleted=&limit=` — any signed-in user (they can read all of this anyway).
pub async fn changes(State(state): State<AppState>, user: AuthUser, Query(q): Query<SyncQuery>) -> AppResult<Json<SyncResponse>> {
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let students_after = Cursor::parse(q.students.as_deref())?;
    let visits_after = Cursor::parse(q.visits.as_deref())?;
    let deleted_after = Cursor::parse(q.deleted.as_deref())?;
    let school = user.school_id;

    // One snapshot, so the lists agree with each other.
    let mut tx = state.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY").execute(&mut *tx).await?;

    let school_row: SyncSchool = sqlx::query_as("SELECT id, name FROM schools WHERE id = $1")
        .bind(school)
        .fetch_one(&mut *tx)
        .await?;
    let accounts: Vec<SyncAccount> = sqlx::query_as(
        "SELECT id, full_name, username, role::text AS role, password_hash
           FROM users
          WHERE school_id = $1 AND is_active AND role::text IN ('admin', 'staff')",
    )
    .bind(school)
    .fetch_all(&mut *tx)
    .await?;
    let sections: Vec<SyncSection> =
        sqlx::query_as("SELECT id, grade_level, section_name FROM sections WHERE school_id = $1")
            .bind(school)
            .fetch_all(&mut *tx)
            .await?;
    let medicines: Vec<SyncMedicine> = sqlx::query_as("SELECT id, name, unit FROM medicines WHERE school_id = $1")
        .bind(school)
        .fetch_all(&mut *tx)
        .await?;
    let staff: Vec<Staff> = sqlx::query_as("SELECT id, full_name FROM users WHERE school_id = $1")
        .bind(school)
        .fetch_all(&mut *tx)
        .await?;

    let students: Vec<SyncStudent> = sqlx::query_as(
        "SELECT id, trim(lrn) AS lrn, first_name, middle_name, last_name, gender, birthdate, section_id,
                created_at, updated_at
           FROM students
          WHERE school_id = $1
            AND ($2::timestamptz IS NULL OR (updated_at, id) > ($2, $3::uuid))
          ORDER BY updated_at, id
          LIMIT $4",
    )
    .bind(school)
    .bind(students_after.map(|c| c.at))
    .bind(students_after.map(|c| c.id))
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;

    let visits: Vec<SyncVisit> = sqlx::query_as(
        "SELECT v.id, v.student_id, v.user_id, v.visited_at, v.complaint, v.treatment, v.created_at, v.updated_at,
                COALESCE((SELECT json_agg(json_build_object('medicine_id', vm.medicine_id, 'quantity', vm.quantity))
                            FROM visit_medicines vm WHERE vm.visit_id = v.id), '[]'::json) AS medicines
           FROM clinic_visits v
          WHERE v.school_id = $1
            AND ($2::timestamptz IS NULL OR (v.updated_at, v.id) > ($2, $3::uuid))
          ORDER BY v.updated_at, v.id
          LIMIT $4",
    )
    .bind(school)
    .bind(visits_after.map(|c| c.at))
    .bind(visits_after.map(|c| c.id))
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;

    let deleted: Vec<Deletion> = sqlx::query_as(
        "SELECT table_name, record_id, deleted_at
           FROM sync_deletions
          WHERE school_id = $1
            AND ($2::timestamptz IS NULL OR (deleted_at, record_id) > ($2, $3::uuid))
          ORDER BY deleted_at, record_id
          LIMIT $4",
    )
    .bind(school)
    .bind(deleted_after.map(|c| c.at))
    .bind(deleted_after.map(|c| c.id))
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    let more = [students.len(), visits.len(), deleted.len()].iter().any(|&n| n as i64 == limit);
    let next = Cursors {
        students: students.last().map(|s| Cursor::format(s.updated_at, s.id)).or(q.students),
        visits: visits.last().map(|v| Cursor::format(v.updated_at, v.id)).or(q.visits),
        deleted: deleted.last().map(|d| Cursor::format(d.deleted_at, d.record_id)).or(q.deleted),
    };
    Ok(Json(SyncResponse { school: school_row, accounts, sections, medicines, staff, students, visits, deleted, next, more }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_round_trip() {
        let at = DateTime::parse_from_rfc3339("2026-11-26T03:04:05.123456Z").unwrap().with_timezone(&Utc);
        let id = Uuid::new_v4();
        let s = Cursor::format(at, id);
        let c = Cursor::parse(Some(&s)).unwrap().unwrap();
        assert_eq!((c.at, c.id), (at, id));
        assert!(Cursor::parse(Some("")).unwrap().is_none());
        assert!(Cursor::parse(None).unwrap().is_none());
        for bad in ["nope", "2026-11-26T03:04:05Z", "2026-11-26T03:04:05Z|x", "x|00000000-0000-0000-0000-000000000000"] {
            assert!(Cursor::parse(Some(bad)).is_err(), "{bad}");
        }
    }
}
