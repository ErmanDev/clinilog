//! Plans, limits and the school's current subscription.
//! This is the single source of truth for plan rules; the desktop app's
//! lock icons are display only and every check is repeated here.

use axum::{extract::State, Json};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{auth::AuthUser, error::AppResult, AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
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
          -- An active row wins over a cancelled one that happens to end later
          -- (switching plans cancels the old row).
          ORDER BY (status = 'active') DESC, end_date DESC
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

// ---------- Prices, payment details and applying a paid plan ----------
//
// Payment is manual: the admin pays by GCash (InstaPay QR), then sends the reference
// number and a receipt screenshot (see `payments.rs`). The plan only starts once the
// superadmin approves the request.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum Term {
    Monthly,
    Yearly,
}

impl Term {
    fn months(self) -> u32 {
        match self {
            Term::Monthly => 1,
            Term::Yearly => 12,
        }
    }
}

/// Prices in Philippine pesos. Change them here; the app shows whatever the server sends.
pub fn price_php(plan: Plan, term: Term) -> i32 {
    match (plan, term) {
        (Plan::Basic, Term::Monthly) => 150,
        (Plan::Basic, Term::Yearly) => 1_300,
        (Plan::Premium, Term::Monthly) => 300,
        (Plan::Premium, Term::Yearly) => 3_600,
    }
}

/// The InstaPay QR admins scan with GCash.
const GCASH_QR_PNG: &[u8] = include_bytes!("../assets/gcash-qr.png");
/// As printed on the QR (masked by GCash).
const GCASH_ACCOUNT_NAME: &str = "MI*****E G.";

#[derive(Serialize)]
pub struct PlanOption {
    pub plan: Plan,
    pub monthly_php: i32,
    pub yearly_php: i32,
    pub limits: Limits,
    pub features: Features,
}

#[derive(Serialize)]
pub struct PaymentInfo {
    pub method: &'static str,
    pub label: &'static str,
    pub account_name: &'static str,
    /// `data:image/png;base64,...`, ready for an `<img>`.
    pub qr_image: String,
}

#[derive(Serialize)]
pub struct PlansResponse {
    pub plans: Vec<PlanOption>,
    pub payment: PaymentInfo,
}

fn qr_data_url() -> String {
    use base64::Engine;
    static URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    URL.get_or_init(|| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(GCASH_QR_PNG)))
        .clone()
}

/// `GET /subscription/plans` (admin) — prices, limits, features and the GCash QR.
pub async fn plans(_user: AuthUser) -> Json<PlansResponse> {
    Json(PlansResponse {
        plans: [Plan::Basic, Plan::Premium]
            .into_iter()
            .map(|p| PlanOption {
                plan: p,
                monthly_php: price_php(p, Term::Monthly),
                yearly_php: price_php(p, Term::Yearly),
                limits: p.limits(),
                features: p.features(),
            })
            .collect(),
        payment: PaymentInfo {
            method: "gcash",
            label: "GCash (InstaPay QR)",
            account_name: GCASH_ACCOUNT_NAME,
            qr_image: qr_data_url(),
        },
    })
}

#[derive(Debug, Serialize)]
pub struct Applied {
    pub subscription_id: Uuid,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    /// `renewal` (same plan, extends the current period), `change` (new plan from today)
    /// or `new` (after expiry, or no plan yet).
    pub kind: &'static str,
}

/// Starts a paid plan for a school. The caller must hold a lock on the school row
/// (`SELECT ... FROM schools ... FOR UPDATE`) in `tx`.
///
/// Same plan while still active: extends from the day after it ends.
/// Different plan, expired, or none: starts today and ends the old plan.
pub async fn apply_plan(
    tx: &mut sqlx::PgConnection,
    db: &PgPool,
    school_id: Uuid,
    plan: Plan,
    term: Term,
) -> AppResult<Applied> {
    let cur = current(db, school_id).await?;
    let today = crate::validate::today_ph();
    let (start, kind) = match (cur.read_only, cur.end_date) {
        (false, Some(end)) if cur.plan == plan => (end.succ_opt().unwrap_or(end), "renewal"),
        (false, Some(_)) => (today, "change"),
        _ => (today, "new"),
    };
    let end = start
        .checked_add_months(chrono::Months::new(term.months()))
        .and_then(|d| d.pred_opt())
        .ok_or_else(|| crate::error::AppError::BadRequest("Date out of range.".into()))?;

    if kind != "renewal" {
        sqlx::query("UPDATE subscriptions SET status = 'cancelled' WHERE school_id = $1 AND status = 'active'")
            .bind(school_id)
            .execute(&mut *tx)
            .await?;
    }
    let subscription_id: Uuid = sqlx::query_scalar(
        "INSERT INTO subscriptions (school_id, plan, start_date, end_date, status)
         VALUES ($1, $2, $3, $4, 'active') RETURNING id",
    )
    .bind(school_id)
    .bind(plan)
    .bind(start)
    .bind(end)
    .fetch_one(&mut *tx)
    .await?;
    Ok(Applied { subscription_id, start_date: start, end_date: end, kind })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices() {
        assert_eq!(price_php(Plan::Basic, Term::Monthly), 150);
        assert_eq!(price_php(Plan::Basic, Term::Yearly), 1_300);
        assert_eq!(price_php(Plan::Premium, Term::Monthly), 300);
        assert_eq!(price_php(Plan::Premium, Term::Yearly), 3_600);
    }

    #[test]
    fn qr_is_a_png() {
        assert!(GCASH_QR_PNG.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(qr_data_url().starts_with("data:image/png;base64,iVBOR"));
    }
}
