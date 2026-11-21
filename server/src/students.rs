//! Student records. All signed-in users can list, view, add and edit;
//! only admins can delete. Every view / create / edit / delete is logged.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{types::Json as SqlJson, PgConnection};
use uuid::Uuid;

use crate::{
    activity::{self, Action},
    auth::AuthUser,
    error::{unique_violation, AppError, AppResult},
    search,
    subscription::ensure_writable,
    validate::{self, FieldErrors},
    AppState,
};

const TABLE: &str = "students";
const MAX_PAGE_SIZE: i64 = 100;

// SQL pieces as macros that expand to string literals, so every query is a
// fixed, parameterized string (no runtime string building).
macro_rules! student_columns {
    () => {
        "st.id, st.lrn, st.first_name, st.middle_name, st.last_name, st.gender, st.birthdate,
         st.created_at, st.updated_at, s.id AS section_id, s.grade_level, s.section_name"
    };
}
macro_rules! student_from {
    () => {
        " FROM students st JOIN sections s ON s.id = st.section_id "
    };
}

// ---------- Shapes ----------

#[derive(Serialize)]
pub struct SectionRef {
    pub id: Uuid,
    pub grade_level: i32,
    pub section_name: String,
}

#[derive(Serialize)]
pub struct Student {
    pub id: Uuid,
    pub lrn: String,
    pub first_name: String,
    pub middle_name: Option<String>,
    pub last_name: String,
    pub gender: String,
    pub birthdate: NaiveDate,
    /// Calculated from birthdate (Philippine date); never stored.
    pub age: i32,
    pub section: SectionRef,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct StudentRow {
    id: Uuid,
    lrn: String,
    first_name: String,
    middle_name: Option<String>,
    last_name: String,
    gender: String,
    birthdate: NaiveDate,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    section_id: Uuid,
    grade_level: i32,
    section_name: String,
}

impl StudentRow {
    fn into_student(self, today: NaiveDate) -> Student {
        Student {
            age: validate::age_on(self.birthdate, today),
            id: self.id,
            // char(12) is fixed width; trim defensively.
            lrn: self.lrn.trim().to_string(),
            first_name: self.first_name,
            middle_name: self.middle_name,
            last_name: self.last_name,
            gender: self.gender,
            birthdate: self.birthdate,
            section: SectionRef { id: self.section_id, grade_level: self.grade_level, section_name: self.section_name },
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

async fn fetch_student(conn: &mut PgConnection, school_id: Uuid, id: Uuid) -> AppResult<Option<Student>> {
    let row: Option<StudentRow> = sqlx::query_as(concat!(
        "SELECT ",
        student_columns!(),
        student_from!(),
        "WHERE st.id = $1 AND st.school_id = $2"
    ))
    .bind(id)
    .bind(school_id)
    .fetch_optional(conn)
    .await?;
    Ok(row.map(|r| r.into_student(validate::today_ph())))
}

/// Locks the student row for this transaction; 404 if it isn't in the user's school.
async fn lock_student(conn: &mut PgConnection, school_id: Uuid, id: Uuid) -> AppResult<()> {
    let found: Option<Uuid> = sqlx::query_scalar("SELECT id FROM students WHERE id = $1 AND school_id = $2 FOR UPDATE")
        .bind(id)
        .bind(school_id)
        .fetch_optional(conn)
        .await?;
    found.map(|_| ()).ok_or_else(not_found)
}

fn not_found() -> AppError {
    AppError::NotFound("Student not found.".into())
}

// ---------- List ----------

#[derive(Deserialize)]
pub struct ListQuery {
    /// Name (partial words, either "first last" or "last, first") or the start of an LRN.
    pub q: Option<String>,
    pub section_id: Option<Uuid>,
    /// Premium (advanced search).
    pub grade_level: Option<i32>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Serialize)]
pub struct StudentPage {
    pub items: Vec<Student>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(sqlx::FromRow)]
struct ListRow {
    #[sqlx(flatten)]
    student: StudentRow,
    total: i64,
}

/// `GET /students?q=&section_id=&grade_level=&page=&page_size=`
/// `grade_level` is an advanced (Premium) filter.
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<StudentPage>> {
    let page = query.page.unwrap_or(1).clamp(1, 100_000);
    let page_size = query.page_size.unwrap_or(25).clamp(1, MAX_PAGE_SIZE);
    if query.grade_level.is_some() {
        crate::subscription::require_advanced_search(&state.db, user.school_id).await?;
    }

    let search::NameTerms { name_pattern, lrn_prefix } = search::name_terms(query.q.as_deref());

    let rows: Vec<ListRow> = sqlx::query_as(concat!(
        "SELECT ",
        student_columns!(),
        ", count(*) OVER () AS total",
        student_from!(),
        "WHERE st.school_id = $1
           AND ($2::uuid IS NULL OR st.section_id = $2)
           AND ($3::text IS NULL
                OR st.lrn LIKE $4
                OR (st.first_name || ' ' || coalesce(st.middle_name || ' ', '') || st.last_name) ILIKE $3 ESCAPE '\\'
                OR (st.last_name || ' ' || st.first_name || coalesce(' ' || st.middle_name, '')) ILIKE $3 ESCAPE '\\')
           AND ($7::int IS NULL OR s.grade_level = $7)
         ORDER BY lower(st.last_name), lower(st.first_name), st.id
         LIMIT $5 OFFSET $6"
    ))
    .bind(user.school_id)
    .bind(query.section_id)
    .bind(&name_pattern)
    .bind(&lrn_prefix)
    .bind(page_size)
    .bind((page - 1) * page_size)
    .bind(query.grade_level)
    .fetch_all(&state.db)
    .await?;

    let total = rows.first().map(|r| r.total).unwrap_or(0);
    let today = validate::today_ph();
    Ok(Json(StudentPage {
        items: rows.into_iter().map(|r| r.student.into_student(today)).collect(),
        total,
        page,
        page_size,
    }))
}

// ---------- LRN availability ----------

#[derive(Deserialize)]
pub struct LrnQuery {
    #[serde(default)]
    pub lrn: String,
    /// The student being edited, whose own LRN doesn't count as taken.
    pub exclude: Option<Uuid>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct LrnOwner {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
}

#[derive(Serialize)]
pub struct LrnCheck {
    pub available: bool,
    /// Who already has it, when taken.
    pub student: Option<LrnOwner>,
}

/// `GET /students/lrn-check?lrn=&exclude=` — lets the form say "already registered"
/// before saving. Save still re-checks (and the database enforces it).
pub async fn lrn_check(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<LrnQuery>,
) -> AppResult<Json<LrnCheck>> {
    let mut e = FieldErrors::default();
    let lrn = validate::lrn(&mut e, &q.lrn);
    e.finish()?;
    let owner: Option<LrnOwner> = sqlx::query_as(
        "SELECT id, first_name, last_name FROM students
          WHERE school_id = $1 AND lrn = $2 AND id IS DISTINCT FROM $3",
    )
    .bind(user.school_id)
    .bind(&lrn)
    .bind(q.exclude)
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(LrnCheck { available: owner.is_none(), student: owner }))
}

// ---------- Detail ----------

#[derive(Serialize, sqlx::FromRow)]
pub struct VisitSummary {
    pub id: Uuid,
    pub visited_at: DateTime<Utc>,
    pub complaint: String,
    pub treatment: String,
    pub attended_by: String,
    /// `[{ name, unit, quantity }]`
    pub medicines: SqlJson<serde_json::Value>,
}

#[derive(Serialize)]
pub struct StudentDetail {
    #[serde(flatten)]
    pub student: Student,
    pub visits: Vec<VisitSummary>,
}

/// `GET /students/:id` — the profile plus full visit history. Logged as a view.
pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> AppResult<Json<StudentDetail>> {
    let mut conn = state.db.acquire().await?;
    let student = fetch_student(&mut conn, user.school_id, id).await?.ok_or_else(not_found)?;

    let visits: Vec<VisitSummary> = sqlx::query_as(
        "SELECT v.id, v.visited_at, v.complaint, v.treatment, u.full_name AS attended_by,
                COALESCE((SELECT json_agg(json_build_object('name', m.name, 'unit', m.unit, 'quantity', vm.quantity)
                                          ORDER BY m.name)
                            FROM visit_medicines vm
                            JOIN medicines m ON m.id = vm.medicine_id
                           WHERE vm.visit_id = v.id), '[]'::json) AS medicines
           FROM clinic_visits v
           JOIN users u ON u.id = v.user_id
          WHERE v.school_id = $1 AND v.student_id = $2
          ORDER BY v.visited_at DESC",
    )
    .bind(user.school_id)
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;

    activity::log(&mut conn, &user, Action::View, TABLE, id).await?;
    Ok(Json(StudentDetail { student, visits }))
}

// ---------- Create / update ----------

#[derive(Deserialize)]
pub struct StudentInput {
    /// Optional client-generated id; makes create safe to retry.
    pub id: Option<Uuid>,
    #[serde(default)]
    pub lrn: String,
    #[serde(default)]
    pub first_name: String,
    pub middle_name: Option<String>,
    #[serde(default)]
    pub last_name: String,
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub birthdate: String,
    pub section_id: Option<Uuid>,
    /// Set after the user confirms the "same name and birthdate" warning.
    #[serde(default)]
    pub allow_duplicate: bool,
}

struct Clean {
    lrn: String,
    first_name: String,
    middle_name: Option<String>,
    last_name: String,
    gender: String,
    birthdate: NaiveDate,
    section_id: Uuid,
}

/// Format checks plus database checks (section belongs to school, LRN unused).
/// `self_id` is the student being edited, so its own LRN doesn't count as taken.
async fn validate_input(
    conn: &mut PgConnection,
    school_id: Uuid,
    self_id: Option<Uuid>,
    input: &StudentInput,
) -> AppResult<Clean> {
    let mut e = FieldErrors::default();
    let lrn = validate::lrn(&mut e, &input.lrn);
    let first_name = validate::name(&mut e, "first_name", "first name", &input.first_name);
    let middle_name = validate::optional_name(&mut e, "middle_name", "middle name", input.middle_name.as_deref());
    let last_name = validate::name(&mut e, "last_name", "last name", &input.last_name);
    let gender = validate::gender(&mut e, &input.gender);
    let birthdate = validate::birthdate(&mut e, &input.birthdate, validate::today_ph());

    match input.section_id {
        None => e.add("section_id", "Choose a section."),
        Some(sid) => {
            let ok: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sections WHERE id = $1 AND school_id = $2)")
                .bind(sid)
                .bind(school_id)
                .fetch_one(&mut *conn)
                .await?;
            if !ok {
                e.add("section_id", "Choose a section from the list.");
            }
        }
    }

    if !e.has("lrn") {
        let taken: Option<(String, String)> = sqlx::query_as(
            "SELECT first_name, last_name FROM students
              WHERE school_id = $1 AND lrn = $2 AND id IS DISTINCT FROM $3",
        )
        .bind(school_id)
        .bind(&lrn)
        .bind(self_id)
        .fetch_optional(&mut *conn)
        .await?;
        if let Some((first, last)) = taken {
            e.add("lrn", format!("This LRN is already registered to {last}, {first}."));
        }
    }

    e.finish()?;
    Ok(Clean {
        lrn,
        first_name,
        middle_name,
        last_name,
        gender,
        birthdate: birthdate.expect("validated"),
        section_id: input.section_id.expect("validated"),
    })
}

/// Warn-don't-block: a 409 the app turns into "Save anyway?" unless already confirmed.
async fn check_duplicate(conn: &mut PgConnection, school_id: Uuid, self_id: Option<Uuid>, c: &Clean) -> AppResult<()> {
    #[derive(Serialize, sqlx::FromRow)]
    struct Match {
        id: Uuid,
        lrn: String,
        first_name: String,
        last_name: String,
        grade_level: i32,
        section_name: String,
    }
    let matches: Vec<Match> = sqlx::query_as(
        "SELECT st.id, st.lrn, st.first_name, st.last_name, s.grade_level, s.section_name
           FROM students st JOIN sections s ON s.id = st.section_id
          WHERE st.school_id = $1
            AND lower(st.first_name) = lower($2)
            AND lower(st.last_name) = lower($3)
            AND st.birthdate = $4
            AND st.id IS DISTINCT FROM $5
          LIMIT 5",
    )
    .bind(school_id)
    .bind(&c.first_name)
    .bind(&c.last_name)
    .bind(c.birthdate)
    .bind(self_id)
    .fetch_all(conn)
    .await?;

    if matches.is_empty() {
        return Ok(());
    }
    Err(AppError::Conflict {
        message: "A student with the same name and birthdate is already registered.".into(),
        code: "possible_duplicate",
        details: Some(json!({ "matches": matches })),
    })
}

fn map_lrn_race(e: sqlx::Error) -> AppError {
    if unique_violation(&e).as_deref() == Some("students_school_id_lrn_key") {
        let mut f = FieldErrors::default();
        f.add("lrn", "This LRN is already registered in your school.");
        return f.finish().unwrap_err();
    }
    e.into()
}

/// `POST /students`
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(input): Json<StudentInput>,
) -> AppResult<(StatusCode, Json<Student>)> {
    let sub = ensure_writable(&state.db, user.school_id).await?;
    let mut tx = state.db.begin().await?;

    // Serialize student creation per school so the plan limit and LRN checks can't race.
    sqlx::query("SELECT 1 FROM schools WHERE id = $1 FOR UPDATE")
        .bind(user.school_id)
        .execute(&mut *tx)
        .await?;

    // Retried upload of a record that already made it: return it unchanged.
    if let Some(id) = input.id {
        let owner: Option<Uuid> = sqlx::query_scalar("SELECT school_id FROM students WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
        match owner {
            Some(s) if s == user.school_id => {
                let existing = fetch_student(&mut tx, user.school_id, id).await?.ok_or_else(not_found)?;
                return Ok((StatusCode::OK, Json(existing)));
            }
            Some(_) => return Err(AppError::BadRequest("Invalid student id.".into())),
            None => {}
        }
    }

    let clean = validate_input(&mut tx, user.school_id, None, &input).await?;

    if let Some(max) = sub.limits.max_students {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM students WHERE school_id = $1")
            .bind(user.school_id)
            .fetch_one(&mut *tx)
            .await?;
        if count >= max {
            return Err(AppError::Forbidden(format!(
                "Your Basic plan allows up to {max} student records. Upgrade to Premium for unlimited records."
            )));
        }
    }

    if !input.allow_duplicate {
        check_duplicate(&mut tx, user.school_id, None, &clean).await?;
    }

    let id = input.id.unwrap_or_else(Uuid::new_v4);
    sqlx::query(
        "INSERT INTO students (id, school_id, section_id, lrn, first_name, middle_name, last_name, gender, birthdate)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(id)
    .bind(user.school_id)
    .bind(clean.section_id)
    .bind(&clean.lrn)
    .bind(&clean.first_name)
    .bind(&clean.middle_name)
    .bind(&clean.last_name)
    .bind(&clean.gender)
    .bind(clean.birthdate)
    .execute(&mut *tx)
    .await
    .map_err(map_lrn_race)?;

    activity::log(&mut tx, &user, Action::Create, TABLE, id).await?;
    let student = fetch_student(&mut tx, user.school_id, id).await?.ok_or_else(not_found)?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(student)))
}

/// `PUT /students/:id`
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(input): Json<StudentInput>,
) -> AppResult<Json<Student>> {
    ensure_writable(&state.db, user.school_id).await?;
    let mut tx = state.db.begin().await?;

    lock_student(&mut tx, user.school_id, id).await?;

    let clean = validate_input(&mut tx, user.school_id, Some(id), &input).await?;

    // Only re-warn about a possible duplicate if the name or birthdate changed;
    // otherwise a duplicate the user already confirmed would warn on every edit.
    let (old_first, old_last, old_birthdate): (String, String, NaiveDate) =
        sqlx::query_as("SELECT first_name, last_name, birthdate FROM students WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    let identity_changed = old_first.to_lowercase() != clean.first_name.to_lowercase()
        || old_last.to_lowercase() != clean.last_name.to_lowercase()
        || old_birthdate != clean.birthdate;
    if identity_changed && !input.allow_duplicate {
        check_duplicate(&mut tx, user.school_id, Some(id), &clean).await?;
    }

    sqlx::query(
        "UPDATE students
            SET section_id = $3, lrn = $4, first_name = $5, middle_name = $6, last_name = $7,
                gender = $8, birthdate = $9
          WHERE id = $1 AND school_id = $2",
    )
    .bind(id)
    .bind(user.school_id)
    .bind(clean.section_id)
    .bind(&clean.lrn)
    .bind(&clean.first_name)
    .bind(&clean.middle_name)
    .bind(&clean.last_name)
    .bind(&clean.gender)
    .bind(clean.birthdate)
    .execute(&mut *tx)
    .await
    .map_err(map_lrn_race)?;

    activity::log(&mut tx, &user, Action::Update, TABLE, id).await?;
    let student = fetch_student(&mut tx, user.school_id, id).await?.ok_or_else(not_found)?;
    tx.commit().await?;
    Ok(Json(student))
}

/// `DELETE /students/:id` (admin). Refused if the student has clinic visits,
/// so health records are never removed by accident.
pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    ensure_writable(&state.db, user.school_id).await?;
    let mut tx = state.db.begin().await?;

    lock_student(&mut tx, user.school_id, id).await?;

    let visits: i64 = sqlx::query_scalar("SELECT count(*) FROM clinic_visits WHERE student_id = $1 AND school_id = $2")
        .bind(id)
        .bind(user.school_id)
        .fetch_one(&mut *tx)
        .await?;
    if visits > 0 {
        return Err(AppError::Conflict {
            message: format!(
                "This student has {visits} clinic visit{} on record, so the student can't be deleted.",
                if visits == 1 { "" } else { "s" }
            ),
            code: "student_has_visits",
            details: None,
        });
    }

    sqlx::query("DELETE FROM students WHERE id = $1 AND school_id = $2")
        .bind(id)
        .bind(user.school_id)
        .execute(&mut *tx)
        .await?;
    activity::log(&mut tx, &user, Action::Delete, TABLE, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
