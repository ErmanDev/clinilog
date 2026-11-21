//! Plans, limits and the school's current subscription.
//! This is the single source of truth for plan rules; the desktop app's
//! lock icons are display only and every check is repeated here.

use axum::{extract::State, Json};
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{auth::AuthUser, error::AppResult, AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum Plan {
    Basic,
    Premium,
}

#[derive(Debug, Clone, Serialize)]
pub struct Limits {
    /// `None` = unlimited.
    pub max_students: Option<i64>,
    pub max_users: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Features {
    pub advanced_search: bool,
    pub detailed_reports: bool,
    pub daily_backups: bool,
}

impl Plan {
    pub fn limits(self) -> Limits {
        match self {
            Plan::Basic => Limits { max_students: Some(500), max_users: 2 },
            Plan::Premium => Limits { max_students: None, max_users: 10 },
        }
    }

    pub fn features(self) -> Features {
        let premium = self == Plan::Premium;
        Features {
            advanced_search: premium,
            detailed_reports: premium,
            daily_backups: premium,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionInfo {
    pub plan: Plan,
    /// `active`, `expired`, `cancelled`, or `none` if the school has no subscription row.
    pub status: String,
    pub start_date: Option<NaiveDate>,
    /// Last day the subscription is valid (inclusive).
    pub end_date: Option<NaiveDate>,
    /// Days left including today; 0 or less once expired.
    pub days_left: Option<i64>,
    /// When true the school can view records but not create, edit or delete them.
    pub read_only: bool,
    pub limits: Limits,
    pub features: Features,
    /// Current counts to compare with `limits`.
    pub usage: Usage,
    /// The server's date, so the app never trusts the PC clock for expiry.
    pub server_date: NaiveDate,
}

#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    pub students: i64,
    /// Only active accounts count toward the plan's account limit.
    pub active_users: i64,
}

#[derive(sqlx::FromRow)]
struct Row {
    plan: Plan,
    status: String,
    start_date: NaiveDate,
    end_date: NaiveDate,
}

/// The school's current subscription: the one that ends latest.
pub async fn current(db: &PgPool, school_id: Uuid) -> AppResult<SubscriptionInfo> {
    let today: NaiveDate = sqlx::query_scalar("SELECT current_date").fetch_one(db).await?;
    let (students, active_users): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM students WHERE school_id = $1),
                (SELECT count(*) FROM users WHERE school_id = $1 AND is_active)",
    )
    .bind(school_id)
    .fetch_one(db)
    .await?;
    let usage = Usage { students, active_users };
    let row: Option<Row> = sqlx::query_as(
        "SELECT plan, status, start_date, end_date
           FROM subscriptions
          WHERE school_id = $1
          ORDER BY end_date DESC
          LIMIT 1",
    )
    .bind(school_id)
    .fetch_optional(db)
    .await?;

    Ok(match row {
        Some(r) => {
            let expired = r.status != "active" || r.end_date < today;
            SubscriptionInfo {
                plan: r.plan,
                status: if expired && r.status == "active" { "expired".into() } else { r.status },
                start_date: Some(r.start_date),
                end_date: Some(r.end_date),
                days_left: Some((r.end_date - today).num_days() + 1),
                read_only: expired,
                limits: r.plan.limits(),
                features: r.plan.features(),
                usage: usage.clone(),
                server_date: today,
            }
        }
        // No subscription at all: viewing still works, nothing can be changed.
        None => SubscriptionInfo {
            plan: Plan::Basic,
            status: "none".into(),
            start_date: None,
            end_date: None,
            days_left: None,
            read_only: true,
            limits: Plan::Basic.limits(),
            features: Plan::Basic.features(),
            usage,
            server_date: today,
        },
    })
}

/// Rejects any create / edit / delete when the subscription is expired or missing.
/// Viewing is never blocked. Returns the subscription so callers can check limits.
pub async fn ensure_writable(db: &PgPool, school_id: Uuid) -> AppResult<SubscriptionInfo> {
    let info = current(db, school_id).await?;
    if info.read_only {
        return Err(crate::error::AppError::Forbidden(
            "Your school's subscription has expired. Records can be viewed but not changed until it is renewed."
                .into(),
        ));
    }
    Ok(info)
}

/// Rejects a request that uses advanced search filters on a Basic plan.
pub async fn require_advanced_search(db: &PgPool, school_id: Uuid) -> AppResult<()> {
    if current(db, school_id).await?.features.advanced_search {
        Ok(())
    } else {
        Err(crate::error::AppError::PremiumRequired(
            "Advanced search filters (date range, complaint and grade) are part of the Premium plan.".into(),
        ))
    }
}

/// `GET /subscription` — any signed-in user. The app calls this on launch.
pub async fn get(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<SubscriptionInfo>> {
    Ok(Json(current(&state.db, user.school_id).await?))
}
