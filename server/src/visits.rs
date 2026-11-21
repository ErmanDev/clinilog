//! Clinic visits. Anyone signed in records visits; the attending staff member
//! is always the logged-in user and the time is set by the server. Staff can
//! edit visits they recorded, admins can edit any; only admins delete.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{types::Json as SqlJson, PgConnection};
use std::collections::HashSet;
use uuid::Uuid;

use crate::{
    activity::{self, Action},
    auth::AuthUser,
    error::{AppError, AppResult},
    search,
    subscription::ensure_writable,
    validate::{self, FieldErrors},
    AppState,
};

const TABLE: &str = "clinic_visits";
const MAX_PAGE_SIZE: i64 = 100;
const MAX_MEDICINE_LINES: usize = 20;
/// Allowed clock difference between a client-supplied visit time and the server.
const CLOCK_SKEW_MINUTES: i64 = 5;
/// Oldest visit time accepted from a client (offline visits are uploaded later).
const MAX_BACKDATE_DAYS: i64 = 30;

// SQL pieces that expand to string literals: every query stays fixed and parameterized.
macro_rules! visit_columns {
    () => {
        "v.id, v.visited_at, v.complaint, v.treatment, v.created_at,
         st.id AS student_id, st.lrn, st.first_name, st.middle_name, st.last_name,
         s.grade_level, s.section_name,
         u.id AS user_id, u.full_name AS attended_by,
         COALESCE((SELECT json_agg(json_build_object(
                              'medicine_id', m.id, 'name', m.name, 'unit', m.unit, 'quantity', vm.quantity)
                              ORDER BY m.name)
                     FROM visit_medicines vm JOIN medicines m ON m.id = vm.medicine_id
                    WHERE vm.visit_id = v.id), '[]'::json) AS medicines"
    };
}
macro_rules! visit_from {
    () => {
        " FROM clinic_visits v
          JOIN students st ON st.id = v.student_id
          JOIN sections s ON s.id = st.section_id
          JOIN users u ON u.id = v.user_id "
    };
}

// ---------- Shapes ----------

#[derive(Serialize)]
pub struct VisitStudent {
    pub id: Uuid,
    pub lrn: String,
    pub first_name: String,
    pub middle_name: Option<String>,
    pub last_name: String,
    pub grade_level: i32,
    pub section_name: String,
}

#[derive(Serialize)]
pub struct Attendant {
    pub id: Uuid,
    pub full_name: String,
}

#[derive(Serialize, Deserialize)]
pub struct MedicineLine {
    pub medicine_id: Uuid,
    pub name: String,
    pub unit: String,
    pub quantity: i32,
}

#[derive(Serialize)]
pub struct Visit {
    pub id: Uuid,
    pub visited_at: DateTime<Utc>,
    pub complaint: String,
    pub treatment: String,
    pub created_at: DateTime<Utc>,
    pub student: VisitStudent,
    pub attended_by: Attendant,
    pub medicines: Vec<MedicineLine>,
}

#[derive(sqlx::FromRow)]
struct VisitRow {
    id: Uuid,
    visited_at: DateTime<Utc>,
    complaint: String,
    treatment: String,
    created_at: DateTime<Utc>,
    student_id: Uuid,
    lrn: String,
    first_name: String,
    middle_name: Option<String>,
    last_name: String,
    grade_level: i32,
    section_name: String,
    user_id: Uuid,
    attended_by: String,
    medicines: SqlJson<Vec<MedicineLine>>,
}

impl From<VisitRow> for Visit {
    fn from(r: VisitRow) -> Self {
        Visit {
            id: r.id,
            visited_at: r.visited_at,
            complaint: r.complaint,
            treatment: r.treatment,
            created_at: r.created_at,
            student: VisitStudent {
                id: r.student_id,
                lrn: r.lrn.trim().to_string(),
                first_name: r.first_name,
                middle_name: r.middle_name,
                last_name: r.last_name,
                grade_level: r.grade_level,
                section_name: r.section_name,
            },
            attended_by: Attendant { id: r.user_id, full_name: r.attended_by },
            medicines: r.medicines.0,
        }
    }
}

async fn fetch_visit(conn: &mut PgConnection, school_id: Uuid, id: Uuid) -> AppResult<Option<Visit>> {
    let row: Option<VisitRow> = sqlx::query_as(concat!(
        "SELECT ",
        visit_columns!(),
        visit_from!(),
        "WHERE v.id = $1 AND v.school_id = $2"
    ))
    .bind(id)
    .bind(school_id)
    .fetch_optional(conn)
    .await?;
    Ok(row.map(Visit::from))
}

fn not_found() -> AppError {
    AppError::NotFound("Visit not found.".into())
}

/// The UTC instants where a Philippine calendar day starts and ends.
pub fn ph_day_bounds(day: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    let ph = FixedOffset::east_opt(8 * 3600).expect("valid offset");
    let start = ph
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).expect("midnight"))
        .single()
        .expect("fixed offset is unambiguous")
        .with_timezone(&Utc);
    (start, start + Duration::days(1))
}

// ---------- List ----------

#[derive(Deserialize)]
pub struct ListQuery {
    /// One Philippine calendar day, `YYYY-MM-DD` (day view on the Visits page; all plans).
    pub date: Option<String>,
    pub student_id: Option<Uuid>,
    /// Basic search: student name or LRN, and section.
    pub q: Option<String>,
    pub section_id: Option<Uuid>,
    /// Advanced search (Premium): grade, date range (inclusive PH days), complaint keywords.
    pub grade_level: Option<i32>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub complaint: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Serialize)]
pub struct VisitPage {
    pub items: Vec<Visit>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(sqlx::FromRow)]
struct ListRow {
    #[sqlx(flatten)]
    visit: VisitRow,
    total: i64,
}

/// `GET /visits` — newest first. See [`ListQuery`] for filters; the advanced ones
/// (grade, date range, complaint) need Premium.
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<VisitPage>> {
    let page = query.page.unwrap_or(1).clamp(1, 100_000);
    let page_size = query.page_size.unwrap_or(25).clamp(1, MAX_PAGE_SIZE);

    let day = search::date_param("Date", query.date.as_deref())?;
    let date_from = search::date_param("From date", query.date_from.as_deref())?;
    let date_to = search::date_param("To date", query.date_to.as_deref())?;
    let complaint_words = search::keywords(query.complaint.as_deref());

    let advanced = query.grade_level.is_some() || date_from.is_some() || date_to.is_some() || complaint_words.is_some();
    if advanced {
        crate::subscription::require_advanced_search(&state.db, user.school_id).await?;
    }
    if let (Some(a), Some(b)) = (date_from, date_to) {
        if a > b {
            return Err(AppError::BadRequest("The 'from' date must be on or before the 'to' date.".into()));
        }
    }

    // Narrowest window from the single day and/or the range, as UTC instants.
    let mut from: Option<DateTime<Utc>> = None;
    let mut to: Option<DateTime<Utc>> = None;
    for start_day in [day, date_from].into_iter().flatten() {
        let s = ph_day_bounds(start_day).0;
        from = Some(from.map_or(s, |f| f.max(s)));
    }
    for end_day in [day, date_to].into_iter().flatten() {
        let e = ph_day_bounds(end_day).1;
        to = Some(to.map_or(e, |t| t.min(e)));
    }

    let search::NameTerms { name_pattern, lrn_prefix } = search::name_terms(query.q.as_deref());

    let rows: Vec<ListRow> = sqlx::query_as(concat!(
        "SELECT ",
        visit_columns!(),
        ", count(*) OVER () AS total",
        visit_from!(),
        "WHERE v.school_id = $1
           AND ($2::timestamptz IS NULL OR v.visited_at >= $2)
           AND ($3::timestamptz IS NULL OR v.visited_at < $3)
           AND ($4::uuid IS NULL OR v.student_id = $4)
           AND ($7::uuid IS NULL OR st.section_id = $7)
           AND ($8::int IS NULL OR s.grade_level = $8)
           AND ($9::text IS NULL
                OR st.lrn LIKE $10
                OR (st.first_name || ' ' || coalesce(st.middle_name || ' ', '') || st.last_name) ILIKE $9 ESCAPE '\\'
                OR (st.last_name || ' ' || st.first_name || coalesce(' ' || st.middle_name, '')) ILIKE $9 ESCAPE '\\')
           AND ($11::text[] IS NULL
                OR NOT EXISTS (SELECT 1 FROM unnest($11::text[]) AS w(word)
                                WHERE v.complaint NOT ILIKE '%' || w.word || '%' ESCAPE '\\'))
         ORDER BY v.visited_at DESC, v.id
         LIMIT $5 OFFSET $6"
    ))
    .bind(user.school_id)
    .bind(from)
    .bind(to)
    .bind(query.student_id)
    .bind(page_size)
    .bind((page - 1) * page_size)
    .bind(query.section_id)
    .bind(query.grade_level)
    .bind(&name_pattern)
    .bind(&lrn_prefix)
    .bind(&complaint_words)
    .fetch_all(&state.db)
    .await?;

    let total = rows.first().map(|r| r.total).unwrap_or(0);
    Ok(Json(VisitPage { items: rows.into_iter().map(|r| r.visit.into()).collect(), total, page, page_size }))
}

/// `GET /visits/:id` — logged as a view.
pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> AppResult<Json<Visit>> {
    let mut conn = state.db.acquire().await?;
    let visit = fetch_visit(&mut conn, user.school_id, id).await?.ok_or_else(not_found)?;
    activity::log(&mut conn, &user, Action::View, TABLE, id).await?;
    Ok(Json(visit))
}

// ---------- Create / update ----------

#[derive(Deserialize)]
pub struct MedicineLineInput {
    pub medicine_id: Option<Uuid>,
    #[serde(default)]
    pub quantity: serde_json::Value,
}

#[derive(Deserialize)]
pub struct VisitInput {
    /// Optional client-generated id; makes create safe to retry.
    pub id: Option<Uuid>,
    pub student_id: Option<Uuid>,
    #[serde(default)]
    pub complaint: String,
    #[serde(default)]
    pub treatment: String,
    #[serde(default)]
    pub medicines: Vec<MedicineLineInput>,
    /// Only for visits recorded offline; otherwise the server uses "now".
    pub visited_at: Option<DateTime<Utc>>,
}

struct CleanLines(Vec<(Uuid, i32)>);

/// Checks complaint, treatment and medicine lines (medicines must belong to the school,
/// whole-number quantities 1–100, no medicine listed twice).
async fn validate_body(
    conn: &mut PgConnection,
    school_id: Uuid,
    input: &VisitInput,
    e: &mut FieldErrors,
) -> AppResult<(String, String, CleanLines)> {
    let complaint = validate::text(e, "complaint", "complaint", &input.complaint, 3, 500);
    let treatment = validate::text(e, "treatment", "treatment", &input.treatment, 1, 500);

    if input.medicines.len() > MAX_MEDICINE_LINES {
        e.add("medicines", format!("A visit can list up to {MAX_MEDICINE_LINES} medicines."));
        return Ok((complaint, treatment, CleanLines(vec![])));
    }

    let ids: Vec<Uuid> = input.medicines.iter().filter_map(|l| l.medicine_id).collect();
    let known: HashSet<Uuid> = sqlx::query_scalar("SELECT id FROM medicines WHERE school_id = $1 AND id = ANY($2)")
        .bind(school_id)
        .bind(&ids)
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .collect();

    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    for (i, line) in input.medicines.iter().enumerate() {
        let qty = validate::quantity(e, format!("medicines.{i}.quantity"), &line.quantity);
        match line.medicine_id {
            None => e.add(format!("medicines.{i}.medicine_id"), "Choose a medicine."),
            Some(id) if !known.contains(&id) => e.add(format!("medicines.{i}.medicine_id"), "Choose a medicine from the list."),
            Some(id) if !seen.insert(id) => e.add(format!("medicines.{i}.medicine_id"), "This medicine is already listed."),
            Some(id) => lines.push((id, qty)),
        }
    }
    Ok((complaint, treatment, CleanLines(lines)))
}

async fn insert_lines(conn: &mut PgConnection, visit_id: Uuid, lines: &CleanLines) -> AppResult<()> {
    for (medicine_id, quantity) in &lines.0 {
        sqlx::query("INSERT INTO visit_medicines (visit_id, medicine_id, quantity) VALUES ($1, $2, $3)")
            .bind(visit_id)
            .bind(medicine_id)
            .bind(quantity)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// `POST /visits`
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(input): Json<VisitInput>,
) -> AppResult<(StatusCode, Json<Visit>)> {
    ensure_writable(&state.db, user.school_id).await?;
    let mut tx = state.db.begin().await?;

    // Retried upload of a visit that already made it: return it unchanged.
    if let Some(id) = input.id {
        let owner: Option<Uuid> = sqlx::query_scalar("SELECT school_id FROM clinic_visits WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
        match owner {
            Some(s) if s == user.school_id => {
                let existing = fetch_visit(&mut tx, user.school_id, id).await?.ok_or_else(not_found)?;
                return Ok((StatusCode::OK, Json(existing)));
            }
            Some(_) => return Err(AppError::BadRequest("Invalid visit id.".into())),
            None => {}
        }
    }

    let mut e = FieldErrors::default();
    match input.student_id {
        None => e.add("student_id", "Choose a student."),
        Some(sid) => {
            let ok: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM students WHERE id = $1 AND school_id = $2)")
                .bind(sid)
                .bind(user.school_id)
                .fetch_one(&mut *tx)
                .await?;
            if !ok {
                e.add("student_id", "Choose a student from the list.");
            }
        }
    }

    let now = Utc::now();
    let visited_at = match input.visited_at {
        None => now,
        Some(t) if t > now + Duration::minutes(CLOCK_SKEW_MINUTES) => {
            e.add("visited_at", "Visit time can't be in the future.");
            now
        }
        Some(t) if t < now - Duration::days(MAX_BACKDATE_DAYS) => {
            e.add("visited_at", format!("Visit time can't be more than {MAX_BACKDATE_DAYS} days ago."));
            now
        }
        Some(t) => t.min(now),
    };

    let (complaint, treatment, lines) = validate_body(&mut tx, user.school_id, &input, &mut e).await?;
    e.finish()?;

    let id = input.id.unwrap_or_else(Uuid::new_v4);
    sqlx::query(
        "INSERT INTO clinic_visits (id, school_id, student_id, user_id, visited_at, complaint, treatment)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(user.school_id)
    .bind(input.student_id)
    .bind(user.id)
    .bind(visited_at)
    .bind(&complaint)
    .bind(&treatment)
    .execute(&mut *tx)
    .await?;
    insert_lines(&mut tx, id, &lines).await?;

    activity::log(&mut tx, &user, Action::Create, TABLE, id).await?;
    let visit = fetch_visit(&mut tx, user.school_id, id).await?.ok_or_else(not_found)?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(visit)))
}

/// `PUT /visits/:id` — complaint, treatment and medicines. The student, time and
/// attending staff don't change.
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(input): Json<VisitInput>,
) -> AppResult<Json<Visit>> {
    ensure_writable(&state.db, user.school_id).await?;
    let mut tx = state.db.begin().await?;

    let found: Option<(Uuid, Uuid)> =
        sqlx::query_as("SELECT user_id, student_id FROM clinic_visits WHERE id = $1 AND school_id = $2 FOR UPDATE")
            .bind(id)
            .bind(user.school_id)
            .fetch_optional(&mut *tx)
            .await?;
    let (recorded_by, student_id) = found.ok_or_else(not_found)?;
    if !user.is_admin() && recorded_by != user.id {
        return Err(AppError::Forbidden(
            "Only the staff member who recorded this visit, or an admin, can edit it.".into(),
        ));
    }

    let mut e = FieldErrors::default();
    if input.student_id.is_some_and(|s| s != student_id) {
        e.add("student_id", "The student on a visit can't be changed. Delete it and record a new visit instead.");
    }
    let (complaint, treatment, lines) = validate_body(&mut tx, user.school_id, &input, &mut e).await?;
    e.finish()?;

    sqlx::query("UPDATE clinic_visits SET complaint = $3, treatment = $4 WHERE id = $1 AND school_id = $2")
        .bind(id)
        .bind(user.school_id)
        .bind(&complaint)
        .bind(&treatment)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM visit_medicines WHERE visit_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    insert_lines(&mut tx, id, &lines).await?;

    activity::log(&mut tx, &user, Action::Update, TABLE, id).await?;
    let visit = fetch_visit(&mut tx, user.school_id, id).await?.ok_or_else(not_found)?;
    tx.commit().await?;
    Ok(Json(visit))
}

/// `DELETE /visits/:id` (admin)
pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    ensure_writable(&state.db, user.school_id).await?;
    let mut tx = state.db.begin().await?;
    let deleted = sqlx::query("DELETE FROM clinic_visits WHERE id = $1 AND school_id = $2")
        .bind(id)
        .bind(user.school_id)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(not_found());
    }
    activity::log(&mut tx, &user, Action::Delete, TABLE, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Dashboard ----------

#[derive(Serialize)]
pub struct Dashboard {
    /// Today's date in the Philippines.
    pub today: NaiveDate,
    pub today_count: i64,
    pub recent: Vec<Visit>,
}

/// `GET /dashboard` — today's visit count and the 10 most recent visits.
pub async fn dashboard(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Dashboard>> {
    let today = validate::today_ph();
    let (from, to) = ph_day_bounds(today);
    let today_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM clinic_visits WHERE school_id = $1 AND visited_at >= $2 AND visited_at < $3",
    )
    .bind(user.school_id)
    .bind(from)
    .bind(to)
    .fetch_one(&state.db)
    .await?;

    let recent: Vec<VisitRow> = sqlx::query_as(concat!(
        "SELECT ",
        visit_columns!(),
        visit_from!(),
        "WHERE v.school_id = $1 ORDER BY v.visited_at DESC, v.id LIMIT 10"
    ))
    .bind(user.school_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(Dashboard { today, today_count, recent: recent.into_iter().map(Visit::from).collect() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ph_day_starts_at_16_00_utc_the_day_before() {
        let (start, end) = ph_day_bounds(NaiveDate::from_ymd_opt(2026, 11, 20).unwrap());
        assert_eq!(start.to_rfc3339(), "2026-11-19T16:00:00+00:00");
        assert_eq!(end.to_rfc3339(), "2026-11-20T16:00:00+00:00");
    }
}
