//! Sections (grade level + name). Everyone signed in can list them;
//! only admins can add, rename or delete.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{unique_violation, AppError, AppResult},
    subscription::ensure_writable,
    validate::{self, FieldErrors},
    AppState,
};

#[derive(Serialize, sqlx::FromRow)]
pub struct Section {
    pub id: Uuid,
    pub grade_level: i32,
    pub section_name: String,
    pub student_count: i64,
}

#[derive(Deserialize)]
pub struct SectionInput {
    /// Optional client-generated id; makes create safe to retry (offline changes).
    #[serde(default)]
    pub id: Option<Uuid>,
    pub grade_level: i32,
    pub section_name: String,
}

/// Shared SELECT, expanded as a literal so every query stays a fixed, parameterized string.
macro_rules! select_section {
    () => {
        "SELECT s.id, s.grade_level, s.section_name,
                (SELECT count(*) FROM students st WHERE st.section_id = s.id) AS student_count
           FROM sections s "
    };
}

/// `GET /sections`
pub async fn list(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Vec<Section>>> {
    let rows = sqlx::query_as(concat!(
        select_section!(),
        "WHERE s.school_id = $1 ORDER BY s.grade_level, lower(s.section_name)"
    ))
    .bind(user.school_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

fn validate(input: &SectionInput) -> AppResult<(i32, String)> {
    let mut errors = FieldErrors::default();
    let grade = validate::grade_level(&mut errors, input.grade_level);
    let name = validate::section_name(&mut errors, &input.section_name);
    errors.finish()?;
    Ok((grade, name))
}

fn duplicate_error() -> AppError {
    let mut e = FieldErrors::default();
    e.add("section_name", "This grade already has a section with that name.");
    e.finish().unwrap_err()
}

/// `POST /sections` (admin)
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(input): Json<SectionInput>,
) -> AppResult<(StatusCode, Json<Section>)> {
    ensure_writable(&state.db, user.school_id).await?;
    if let Some(existing) = already_created(&state, user.school_id, input.id).await? {
        return Ok((StatusCode::OK, Json(existing)));
    }
    let (grade, name) = validate(&input)?;

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO sections (id, school_id, grade_level, section_name) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(input.id.unwrap_or_else(Uuid::new_v4))
    .bind(user.school_id)
    .bind(grade)
    .bind(&name)
    .fetch_one(&state.db)
    .await
    .map_err(|e| if unique_violation(&e).is_some() { duplicate_error() } else { e.into() })?;

    Ok((StatusCode::CREATED, Json(fetch(&state, user.school_id, id).await?)))
}

/// `PUT /sections/:id` (admin)
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(input): Json<SectionInput>,
) -> AppResult<Json<Section>> {
    ensure_writable(&state.db, user.school_id).await?;
    let (grade, name) = validate(&input)?;

    let updated = sqlx::query(
        "UPDATE sections SET grade_level = $3, section_name = $4 WHERE id = $1 AND school_id = $2",
    )
    .bind(id)
    .bind(user.school_id)
    .bind(grade)
    .bind(&name)
    .execute(&state.db)
    .await
    .map_err(|e| if unique_violation(&e).is_some() { duplicate_error() } else { e.into() })?;
    if updated.rows_affected() == 0 {
        return Err(not_found());
    }
    Ok(Json(fetch(&state, user.school_id, id).await?))
}

/// `DELETE /sections/:id` (admin). Refused while students are assigned to it.
pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    ensure_writable(&state.db, user.school_id).await?;
    let section = fetch(&state, user.school_id, id).await?;
    if section.student_count > 0 {
        return Err(AppError::Conflict {
            message: format!(
                "Grade {} – {} still has {} student{}. Move them to another section first.",
                section.grade_level,
                section.section_name,
                section.student_count,
                if section.student_count == 1 { "" } else { "s" }
            ),
            code: "section_in_use",
            details: None,
        });
    }
    // The FK on students also blocks this if a student was added in the meantime.
    sqlx::query("DELETE FROM sections WHERE id = $1 AND school_id = $2")
        .bind(id)
        .bind(user.school_id)
        .execute(&state.db)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db) if db.code().as_deref() == Some("23503") => AppError::Conflict {
                message: "This section still has students. Move them to another section first.".into(),
                code: "section_in_use",
                details: None,
            },
            _ => e.into(),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// A retried create whose first attempt made it: the section as it is now.
async fn already_created(state: &AppState, school_id: Uuid, id: Option<Uuid>) -> AppResult<Option<Section>> {
    let Some(id) = id else { return Ok(None) };
    let owner: Option<Uuid> = sqlx::query_scalar("SELECT school_id FROM sections WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    match owner {
        None => Ok(None),
        Some(s) if s == school_id => Ok(Some(fetch(state, school_id, id).await?)),
        Some(_) => Err(AppError::BadRequest("Invalid section id.".into())),
    }
}

async fn fetch(state: &AppState, school_id: Uuid, id: Uuid) -> AppResult<Section> {
    sqlx::query_as(concat!(select_section!(), "WHERE s.id = $1 AND s.school_id = $2"))
        .bind(id)
        .bind(school_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(not_found)
}

fn not_found() -> AppError {
    AppError::NotFound("Section not found.".into())
}
