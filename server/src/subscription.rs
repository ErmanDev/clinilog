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

// ---------- Mock checkout (no real payment) ----------
//
// Clinilog has no payment provider. Admins "buy" a plan through a demo checkout:
// no money moves and no payment details are collected. Prices are samples.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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

/// Sample prices in Philippine pesos. Change them here; the app shows whatever the server sends.
fn price_php(plan: Plan, term: Term) -> i64 {
    match (plan, term) {
        (Plan::Basic, Term::Monthly) => 500,
        (Plan::Basic, Term::Yearly) => 5_000,
        (Plan::Premium, Term::Monthly) => 1_500,
        (Plan::Premium, Term::Yearly) => 15_000,
    }
}

const PAYMENT_METHODS: &[(&str, &str)] =
    &[("gcash", "GCash"), ("maya", "Maya"), ("card", "Credit / debit card"), ("bank", "Bank transfer")];

#[derive(Serialize)]
pub struct PlanOption {
    pub plan: Plan,
    pub monthly_php: i64,
    pub yearly_php: i64,
    pub limits: Limits,
    pub features: Features,
}

#[derive(Serialize)]
pub struct PlansResponse {
    pub plans: Vec<PlanOption>,
    /// `[{ id, label }]`
    pub payment_methods: Vec<serde_json::Value>,
    /// Always true: nothing is charged.
    pub demo: bool,
}

/// `GET /subscription/plans` (admin) — prices, limits and features for the plan picker.
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
        payment_methods: PAYMENT_METHODS.iter().map(|(id, label)| serde_json::json!({ "id": id, "label": label })).collect(),
        demo: true,
    })
}

#[derive(Deserialize)]
pub struct CheckoutInput {
    pub plan: Plan,
    pub term: Term,
    pub payment_method: String,
}

#[derive(Serialize)]
pub struct Receipt {
    /// Shown to the admin only; not stored.
    pub reference: String,
    pub plan: Plan,
    pub term: Term,
    pub amount_php: i64,
    pub payment_method: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    /// `renewal` (same plan, extends the current period), `change` (new plan from today)
    /// or `new` (after expiry, or no plan yet).
    pub kind: &'static str,
    pub demo: bool,
    pub subscription: SubscriptionInfo,
}

/// `POST /subscription/checkout` (admin) — demo purchase of a plan. Works while the
/// subscription is expired (that's when schools renew). No money moves.
pub async fn checkout(
    State(state): State<AppState>,
    admin: AuthUser,
    Json(input): Json<CheckoutInput>,
) -> AppResult<Json<Receipt>> {
    let method = PAYMENT_METHODS
        .iter()
        .find(|(id, _)| *id == input.payment_method)
        .map(|(_, label)| label.to_string())
        .ok_or_else(|| crate::error::AppError::BadRequest("Choose a payment method.".into()))?;

    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT 1 FROM schools WHERE id = $1 FOR UPDATE")
        .bind(admin.school_id)
        .execute(&mut *tx)
        .await?;
    let cur = current(&state.db, admin.school_id).await?;
    let today = crate::validate::today_ph();

    // Same plan while still active: extend from the day after it ends.
    // Different plan, expired, or none: start today and end the old plan.
    let (start, kind) = match (cur.read_only, cur.end_date) {
        (false, Some(end)) if cur.plan == input.plan => (end.succ_opt().unwrap_or(end), "renewal"),
        (false, Some(_)) => (today, "change"),
        _ => (today, "new"),
    };
    let end = start
        .checked_add_months(chrono::Months::new(input.term.months()))
        .and_then(|d| d.pred_opt())
        .ok_or_else(|| crate::error::AppError::BadRequest("Date out of range.".into()))?;

    if kind != "renewal" {
        sqlx::query("UPDATE subscriptions SET status = 'cancelled' WHERE school_id = $1 AND status = 'active'")
            .bind(admin.school_id)
            .execute(&mut *tx)
            .await?;
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO subscriptions (school_id, plan, start_date, end_date, status)
         VALUES ($1, $2, $3, $4, 'active') RETURNING id",
    )
    .bind(admin.school_id)
    .bind(input.plan)
    .bind(start)
    .bind(end)
    .fetch_one(&mut *tx)
    .await?;
    crate::activity::log(&mut tx, &admin, crate::activity::Action::Create, "subscriptions", id).await?;
    tx.commit().await?;

    let reference = format!("DEMO-{}", &id.simple().to_string()[..8].to_uppercase());
    tracing::info!(school_id = %admin.school_id, plan = ?input.plan, term = ?input.term, kind, "demo checkout");
    Ok(Json(Receipt {
        reference,
        plan: input.plan,
        term: input.term,
        amount_php: price_php(input.plan, input.term),
        payment_method: method,
        start_date: start,
        end_date: end,
        kind,
        demo: true,
        subscription: current(&state.db, admin.school_id).await?,
    }))
}
