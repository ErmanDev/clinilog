//! Audit trail (§5): every view, create, edit or delete of a student or visit record,
//! plus changes to user accounts. Admins read it through `GET /activity`.

use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{AppError, AppResult},
    search,
    visits::ph_day_bounds,
    AppState,
};

const MAX_PAGE_SIZE: i64 = 100;
const TABLES: &[&str] = &["students", "clinic_visits", "users"];
const ACTIONS: &[&str] = &["view", "create", "update", "delete"];

#[derive(Deserialize)]
pub struct ListQuery {
    pub user_id: Option<Uuid>,
    pub action: Option<String>,
    /// `students`, `clinic_visits` or `users`.
    pub table: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Entry {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub action: String,
    pub table_name: String,
    pub record_id: Uuid,
    /// Readable name of the record, or `None` if it has since been deleted.
    pub record_label: Option<String>,
    pub user_id: Uuid,
    pub user_full_name: String,
    pub user_username: String,
}

#[derive(Serialize)]
pub struct EntryPage {
    pub items: Vec<Entry>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(sqlx::FromRow)]
struct Row {
    #[sqlx(flatten)]
    entry: Entry,
    total: i64,
}

fn one_of(value: Option<&str>, allowed: &[&str], label: &str) -> AppResult<Option<String>> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) if allowed.contains(&v) => Ok(Some(v.to_string())),
        Some(_) => Err(AppError::BadRequest(format!("Unknown {label}."))),
    }
}

/// `GET /activity?user_id=&action=&table=&date_from=&date_to=&page=&page_size=` (admin) — newest first.
pub async fn list(State(state): State<AppState>, admin: AuthUser, Query(q): Query<ListQuery>) -> AppResult<Json<EntryPage>> {
    let page = q.page.unwrap_or(1).clamp(1, 100_000);
    let page_size = q.page_size.unwrap_or(50).clamp(1, MAX_PAGE_SIZE);
    let action = one_of(q.action.as_deref(), ACTIONS, "action")?;
    let table = one_of(q.table.as_deref(), TABLES, "record type")?;
    let from = search::date_param("From date", q.date_from.as_deref())?.map(|d| ph_day_bounds(d).0);
    let to = search::date_param("To date", q.date_to.as_deref())?.map(|d| ph_day_bounds(d).1);
    if let (Some(a), Some(b)) = (from, to) {
        if a >= b {
            return Err(AppError::BadRequest("The 'from' date must be on or before the 'to' date.".into()));
        }
    }

    let rows: Vec<Row> = sqlx::query_as(
        "SELECT a.id, a.created_at, a.action, a.table_name, a.record_id,
                u.id AS user_id, u.full_name AS user_full_name, u.username AS user_username,
                CASE a.table_name
                  WHEN 'students' THEN
                    (SELECT st.last_name || ', ' || st.first_name || ' (LRN ' || trim(st.lrn) || ')'
                       FROM students st WHERE st.id = a.record_id AND st.school_id = a.school_id)
                  WHEN 'clinic_visits' THEN
                    (SELECT 'Visit of ' || st.last_name || ', ' || st.first_name || ' on '
                            || to_char(v.visited_at AT TIME ZONE 'Asia/Manila', 'Mon DD, YYYY HH12:MI AM')
                       FROM clinic_visits v JOIN students st ON st.id = v.student_id
                      WHERE v.id = a.record_id AND v.school_id = a.school_id)
                  WHEN 'users' THEN
                    (SELECT u2.full_name || ' (' || u2.username || ')'
                       FROM users u2 WHERE u2.id = a.record_id AND u2.school_id = a.school_id)
                END AS record_label,
                count(*) OVER () AS total
           FROM activity_log a
           JOIN users u ON u.id = a.user_id
          WHERE a.school_id = $1
            AND ($2::uuid IS NULL OR a.user_id = $2)
            AND ($3::text IS NULL OR a.action = $3)
            AND ($4::text IS NULL OR a.table_name = $4)
            AND ($5::timestamptz IS NULL OR a.created_at >= $5)
            AND ($6::timestamptz IS NULL OR a.created_at < $6)
          ORDER BY a.created_at DESC, a.id
          LIMIT $7 OFFSET $8",
    )
    .bind(admin.school_id)
    .bind(q.user_id)
    .bind(&action)
    .bind(&table)
    .bind(from)
    .bind(to)
    .bind(page_size)
    .bind((page - 1) * page_size)
    .fetch_all(&state.db)
    .await?;

    let total = rows.first().map(|r| r.total).unwrap_or(0);
    Ok(Json(EntryPage { items: rows.into_iter().map(|r| r.entry).collect(), total, page, page_size }))
}

#[derive(Clone, Copy)]
pub enum Action {
    View,
    Create,
    Update,
    Delete,
}

impl Action {
    fn as_str(self) -> &'static str {
        match self {
            Action::View => "view",
            Action::Create => "create",
            Action::Update => "update",
            Action::Delete => "delete",
        }
    }
}

/// Writes one log row. Pass the same transaction as the change so both commit together.
pub async fn log(conn: &mut PgConnection, user: &AuthUser, action: Action, table: &str, record_id: Uuid) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO activity_log (school_id, user_id, action, table_name, record_id)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user.school_id)
    .bind(user.id)
    .bind(action.as_str())
    .bind(table)
    .bind(record_id)
    .execute(conn)
    .await?;
    Ok(())
}
