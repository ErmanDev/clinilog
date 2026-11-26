//! Medicines the clinic gives out. Everyone signed in can list them (to pick on
//! a visit); only admins can add, edit or delete.

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
    validate::FieldErrors,
    AppState,
};

#[derive(Serialize, sqlx::FromRow)]
pub struct Medicine {
    pub id: Uuid,
    pub name: String,
    pub unit: String,
    /// How many visits used it; a used medicine can't be deleted.
    pub visit_count: i64,
}

#[derive(Deserialize)]
pub struct MedicineInput {
    /// Optional client-generated id; makes create safe to retry (offline changes).
    #[serde(default)]
    pub id: Option<Uuid>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub unit: String,
}

macro_rules! select_medicine {
    () => {
        "SELECT m.id, m.name, m.unit,
                (SELECT count(*) FROM visit_medicines vm WHERE vm.medicine_id = m.id) AS visit_count
           FROM medicines m "
    };
}

/// `GET /medicines`
pub async fn list(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Vec<Medicine>>> {
    let rows = sqlx::query_as(concat!(select_medicine!(), "WHERE m.school_id = $1 ORDER BY lower(m.name)"))
        .bind(user.school_id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(rows))
}

fn short_text(errors: &mut FieldErrors, field: &'static str, label: &str, value: &str, max: usize) -> String {
    let v = value.trim().to_string();
    let len = v.chars().count();
    if len == 0 {
        errors.add(field, format!("Enter the {label}."));
    } else if len > max {
        errors.add(field, format!("Must be {max} characters or fewer."));
    } else if v.chars().any(char::is_control) {
        errors.add(field, "Has characters that aren't allowed.");
    }
    v
}

/// Validates, and checks the name isn't already used (ignoring case) by another medicine.
async fn validate(state: &AppState, school_id: Uuid, self_id: Option<Uuid>, input: &MedicineInput) -> AppResult<(String, String)> {
    let mut e = FieldErrors::default();
    let name = short_text(&mut e, "name", "medicine name", &input.name, 100);
    let unit = short_text(&mut e, "unit", "unit (e.g. tablet, ml, sachet)", &input.unit, 30);
    if !e.has("name") {
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM medicines
                             WHERE school_id = $1 AND lower(name) = lower($2) AND id IS DISTINCT FROM $3)",
        )
        .bind(school_id)
        .bind(&name)
        .bind(self_id)
        .fetch_one(&state.db)
        .await?;
        if taken {
            e.add("name", "A medicine with this name already exists.");
        }
    }
    e.finish()?;
    Ok((name, unit))
}

fn map_unique(e: sqlx::Error) -> AppError {
    if unique_violation(&e).is_some() {
        let mut f = FieldErrors::default();
        f.add("name", "A medicine with this name already exists.");
        return f.finish().unwrap_err();
    }
    e.into()
}

/// `POST /medicines` (admin)
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(input): Json<MedicineInput>,
) -> AppResult<(StatusCode, Json<Medicine>)> {
    ensure_writable(&state.db, user.school_id).await?;
    if let Some(id) = input.id {
        let owner: Option<Uuid> = sqlx::query_scalar("SELECT school_id FROM medicines WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
        match owner {
            Some(s) if s == user.school_id => return Ok((StatusCode::OK, Json(fetch(&state, user.school_id, id).await?))),
            Some(_) => return Err(AppError::BadRequest("Invalid medicine id.".into())),
            None => {}
        }
    }
    let (name, unit) = validate(&state, user.school_id, None, &input).await?;
    let id: Uuid = sqlx::query_scalar("INSERT INTO medicines (id, school_id, name, unit) VALUES ($1, $2, $3, $4) RETURNING id")
        .bind(input.id.unwrap_or_else(Uuid::new_v4))
        .bind(user.school_id)
        .bind(&name)
        .bind(&unit)
        .fetch_one(&state.db)
        .await
        .map_err(map_unique)?;
    Ok((StatusCode::CREATED, Json(fetch(&state, user.school_id, id).await?)))
}

/// `PUT /medicines/:id` (admin)
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(input): Json<MedicineInput>,
) -> AppResult<Json<Medicine>> {
    ensure_writable(&state.db, user.school_id).await?;
    fetch(&state, user.school_id, id).await?;
    let (name, unit) = validate(&state, user.school_id, Some(id), &input).await?;
    sqlx::query("UPDATE medicines SET name = $3, unit = $4 WHERE id = $1 AND school_id = $2")
        .bind(id)
        .bind(user.school_id)
        .bind(&name)
        .bind(&unit)
        .execute(&state.db)
        .await
        .map_err(map_unique)?;
    Ok(Json(fetch(&state, user.school_id, id).await?))
}

/// `DELETE /medicines/:id` (admin). Refused once a visit has used it, to keep history intact.
pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    ensure_writable(&state.db, user.school_id).await?;
    let m = fetch(&state, user.school_id, id).await?;
    let in_use = || AppError::Conflict {
        message: format!("{} was given in past visits, so it can't be deleted. You can rename it instead.", m.name),
        code: "medicine_in_use",
        details: None,
    };
    if m.visit_count > 0 {
        return Err(in_use());
    }
    sqlx::query("DELETE FROM medicines WHERE id = $1 AND school_id = $2")
        .bind(id)
        .bind(user.school_id)
        .execute(&state.db)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db) if db.code().as_deref() == Some("23503") => in_use(),
            _ => e.into(),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch(state: &AppState, school_id: Uuid, id: Uuid) -> AppResult<Medicine> {
    sqlx::query_as(concat!(select_medicine!(), "WHERE m.id = $1 AND m.school_id = $2"))
        .bind(id)
        .bind(school_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("Medicine not found.".into()))
}
