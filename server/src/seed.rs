//! First-time setup: one school (VNHS), a 12-month Basic subscription, one admin
//! account, sample sections and common medicines.
//!
//! Runs from `cargo run --bin seed`, or automatically when the server starts with
//! `SEED_ADMIN_PASSWORD` set (handy on Render's free tier, which has no shell).
//! Safe to run more than once: if the school already exists, nothing changes.

use anyhow::{bail, Context, Result};
use chrono::{Months, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::password;

pub const SCHOOL_NAME: &str = "Valencia National High School";
const SCHOOL_CODE: &str = "VNHS";
const SCHOOL_ADDRESS: &str = "Valencia City, Bukidnon";
const ADMIN_FULL_NAME: &str = "Clinic Administrator";

/// (grade_level, section_name) — sample data only; real sections are managed in the app.
const SAMPLE_SECTIONS: &[(i32, &str)] = &[
    (7, "Sampaguita"),
    (7, "Rosal"),
    (8, "Narra"),
    (8, "Molave"),
    (9, "Rizal"),
    (9, "Mabini"),
    (10, "Diamond"),
    (10, "Emerald"),
    (11, "STEM-A"),
    (11, "HUMSS-A"),
    (12, "STEM-A"),
    (12, "ABM-A"),
];

/// (name, unit) — common school-clinic stock; admins edit the list in Settings.
/// New schools made by the superadmin start with this list too.
pub const SAMPLE_MEDICINES: &[(&str, &str)] = &[
    ("Paracetamol 500 mg", "tablet"),
    ("Mefenamic Acid 500 mg", "capsule"),
    ("Cetirizine 10 mg", "tablet"),
    ("Loperamide 2 mg", "capsule"),
    ("Oral Rehydration Salts", "sachet"),
    ("Povidone-Iodine", "ml"),
];

pub struct Outcome {
    pub school_id: Uuid,
    /// false = the school already existed and nothing was changed.
    pub created: bool,
    pub username: String,
}

/// `SEED_ADMIN_USERNAME` (default `admin`) and `SEED_ADMIN_PASSWORD`, if the password is set.
pub fn admin_from_env() -> Option<(String, String)> {
    let password = std::env::var("SEED_ADMIN_PASSWORD").ok().filter(|p| !p.is_empty())?;
    let username = std::env::var("SEED_ADMIN_USERNAME").unwrap_or_else(|_| "admin".into());
    Some((username, password))
}

pub async fn run(pool: &PgPool, username: &str, admin_password: &str) -> Result<Outcome> {
    // Already set up: do nothing (and don't even look at the seed settings).
    let existing: Option<Uuid> = sqlx::query_scalar("SELECT id FROM schools WHERE name = $1")
        .bind(SCHOOL_NAME)
        .fetch_optional(pool)
        .await?;
    if let Some(school_id) = existing {
        return Ok(Outcome { school_id, created: false, username: username.to_string() });
    }

    validate_username(username)?;
    validate_password(admin_password)?;
    let password_hash = password::hash(admin_password)?;

    let mut tx = pool.begin().await?;
    let school_id: Uuid = sqlx::query_scalar("INSERT INTO schools (name, address, code) VALUES ($1, $2, $3) RETURNING id")
        .bind(SCHOOL_NAME)
        .bind(SCHOOL_ADDRESS)
        .bind(SCHOOL_CODE)
        .fetch_one(&mut *tx)
        .await?;

    let start = Utc::now().date_naive();
    let end = start.checked_add_months(Months::new(12)).context("date overflow")?;
    sqlx::query(
        "INSERT INTO subscriptions (school_id, plan, start_date, end_date, status)
         VALUES ($1, 'basic', $2, $3, 'active')",
    )
    .bind(school_id)
    .bind(start)
    .bind(end)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO users (school_id, full_name, username, password_hash, role)
         VALUES ($1, $2, $3, $4, 'admin')",
    )
    .bind(school_id)
    .bind(ADMIN_FULL_NAME)
    .bind(username)
    .bind(&password_hash)
    .execute(&mut *tx)
    .await?;

    for (grade, name) in SAMPLE_SECTIONS {
        sqlx::query("INSERT INTO sections (school_id, grade_level, section_name) VALUES ($1, $2, $3)")
            .bind(school_id)
            .bind(grade)
            .bind(name)
            .execute(&mut *tx)
            .await?;
    }
    for (name, unit) in SAMPLE_MEDICINES {
        sqlx::query("INSERT INTO medicines (school_id, name, unit) VALUES ($1, $2, $3)")
            .bind(school_id)
            .bind(name)
            .bind(unit)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    Ok(Outcome { school_id, created: true, username: username.to_string() })
}

fn validate_username(u: &str) -> Result<()> {
    if !(4..=30).contains(&u.len()) || !u.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!("SEED_ADMIN_USERNAME must be 4-30 chars: letters, digits, underscore");
    }
    Ok(())
}

fn validate_password(p: &str) -> Result<()> {
    if p.chars().count() < 8 || !p.chars().any(|c| c.is_alphabetic()) || !p.chars().any(|c| c.is_ascii_digit()) {
        bail!("SEED_ADMIN_PASSWORD must be at least 8 chars, with a letter and a number");
    }
    Ok(())
}
