//! Subscription payment requests.
//!
//! 1. A school admin pays by GCash (InstaPay QR) and sends the reference number plus a
//!    receipt screenshot: `POST /subscription/requests`. Nothing changes yet.
//! 2. The superadmin compares it with the payment received and approves or rejects it.
//!    Approving starts the plan (same rules as renewing or changing plans).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use base64::Engine;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    activity::{self, Action},
    auth::{AuthUser, SuperadminAuth},
    error::{unique_violation, AppError, AppResult},
    subscription::{self, is_first_payment, price_php, Plan, SubscriptionInfo, Term},
    validate::FieldErrors,
    AppState,
};

const TABLE: &str = "subscription_requests";
/// Largest receipt image accepted, after decoding. The app shrinks photos before sending.
pub const MAX_RECEIPT_BYTES: usize = 3 * 1024 * 1024;
/// Request body limit for the upload route (base64 adds about a third).
pub const UPLOAD_BODY_LIMIT: usize = 5 * 1024 * 1024;
const STATUSES: &[&str] = &["pending", "approved", "rejected", "cancelled"];

/// Letters and digits only, upper-case: "1234 567 890" and "1234-567-890" match.
pub fn normalize_reference(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_uppercase)
        .collect()
}

fn valid_reference(r: &str) -> bool {
    (6..=30).contains(&r.len()) && r.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The image type from its first bytes (the file name and declared type are ignored).
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Accepts `data:image/...;base64,XXXX` or plain base64.
fn decode_receipt(input: &str) -> Result<(Vec<u8>, &'static str), &'static str> {
    let b64 = match input.trim().strip_prefix("data:") {
        Some(rest) => rest.split_once(',').map(|(_, d)| d).ok_or("The receipt image couldn't be read.")?,
        None => input.trim(),
    };
    if b64.is_empty() {
        return Err("Upload a screenshot or photo of your GCash receipt.");
    }
    if b64.len() > MAX_RECEIPT_BYTES / 3 * 4 + 8 {
        return Err("The receipt image is too large (3 MB at most).");
    }
    let cleaned: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .map_err(|_| "The receipt image couldn't be read.")?;
    if bytes.len() > MAX_RECEIPT_BYTES {
        return Err("The receipt image is too large (3 MB at most).");
    }
    let mime = sniff_image(&bytes).ok_or("The receipt must be a PNG, JPG or WEBP image.")?;
    if bytes.len() < 1024 {
        return Err("The receipt image is too small to read. Upload the full screenshot.");
    }
    Ok((bytes, mime))
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

// ---------- School admin ----------

#[derive(Deserialize)]
pub struct CreateInput {
    pub plan: Plan,
    /// Sent only by app versions from before one-time payments, which show old prices.
    #[serde(default)]
    pub term: Option<Term>,
    #[serde(default)]
    pub reference_no: String,
    /// Data URL or base64 of the receipt image.
    #[serde(default)]
    pub receipt_image: String,
}

/// A request as the school admin sees it (no image).
#[derive(Serialize, sqlx::FromRow)]
pub struct SchoolRequest {
    pub id: Uuid,
    pub plan: Plan,
    pub term: Term,
    pub amount_php: i32,
    pub payment_method: String,
    pub reference_no: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub review_note: Option<String>,
    pub requested_by_name: String,
    /// The plan period, once approved.
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
}

async fn school_request(db: &sqlx::PgPool, school_id: Uuid, id: Uuid) -> AppResult<SchoolRequest> {
    sqlx::query_as(
        "SELECT r.id, r.plan, r.term, r.amount_php, r.payment_method, r.reference_no,
                r.status, r.created_at, r.reviewed_at, r.review_note,
                u.full_name AS requested_by_name, s.start_date, s.end_date
           FROM subscription_requests r
           JOIN users u ON u.id = r.requested_by
           LEFT JOIN subscriptions s ON s.id = r.subscription_id
          WHERE r.id = $1 AND r.school_id = $2",
    )
    .bind(id)
    .bind(school_id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::NotFound("Payment request not found.".into()))
}

/// `POST /subscription/requests` (admin) — send a payment for review. Allowed while the
/// subscription is expired (that's when schools renew).
pub async fn create(
    State(state): State<AppState>,
    admin: AuthUser,
    Json(input): Json<CreateInput>,
) -> AppResult<(StatusCode, Json<SchoolRequest>)> {
    let mut e = FieldErrors::default();
    let reference = normalize_reference(&input.reference_no);
    if reference.is_empty() {
        e.add("reference_no", "Enter the reference number from your GCash receipt.");
    } else if !valid_reference(&reference) {
        e.add("reference_no", "The reference number should be 6–30 letters or numbers, as shown on the receipt.");
    }
    let receipt = match decode_receipt(&input.receipt_image) {
        Ok(r) => Some(r),
        Err(msg) => {
            e.add("receipt_image", msg);
            None
        }
    };
    if input.term.is_some() {
        return Err(AppError::BadRequest(
            "Prices have changed. Update Clinilog to the latest version, then send your payment again.".into(),
        ));
    }
    e.finish()?;
    let (bytes, mime) = receipt.expect("checked above");

    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT 1 FROM schools WHERE id = $1 FOR UPDATE")
        .bind(admin.school_id)
        .execute(&mut *tx)
        .await?;
    // Decided under the school lock, so two requests can't both get the first-payment price.
    let amount = price_php(input.plan, is_first_payment(&mut *tx, admin.school_id).await?);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO subscription_requests
             (school_id, requested_by, plan, term, amount_php, reference_no, receipt_image, receipt_mime)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind(admin.school_id)
    .bind(admin.id)
    .bind(input.plan)
    .bind(Term::Yearly)
    .bind(amount)
    .bind(&reference)
    .bind(&bytes)
    .bind(mime)
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| match unique_violation(&err).as_deref() {
        Some("subscription_requests_one_pending") => AppError::Conflict {
            message: "Your school already has a payment waiting for review. Wait for it, or cancel it first.".into(),
            code: "request_pending",
            details: None,
        },
        Some("subscription_requests_reference_key") => {
            let mut f = FieldErrors::default();
            f.add("reference_no", "This reference number was already sent for another payment.");
            f.finish().unwrap_err()
        }
        _ => err.into(),
    })?;
    activity::log(&mut tx, &admin, Action::Create, TABLE, id).await?;
    tx.commit().await?;
    tracing::info!(school_id = %admin.school_id, request_id = %id, plan = ?input.plan, amount, "payment request sent");
    Ok((StatusCode::CREATED, Json(school_request(&state.db, admin.school_id, id).await?)))
}

/// `GET /subscription/requests` (admin) — the school's payment requests, newest first.
pub async fn list_for_school(State(state): State<AppState>, admin: AuthUser) -> AppResult<Json<Vec<SchoolRequest>>> {
    let rows = sqlx::query_as(
        "SELECT r.id, r.plan, r.term, r.amount_php, r.payment_method, r.reference_no,
                r.status, r.created_at, r.reviewed_at, r.review_note,
                u.full_name AS requested_by_name, s.start_date, s.end_date
           FROM subscription_requests r
           JOIN users u ON u.id = r.requested_by
           LEFT JOIN subscriptions s ON s.id = r.subscription_id
          WHERE r.school_id = $1
          ORDER BY r.created_at DESC
          LIMIT 50",
    )
    .bind(admin.school_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// `POST /subscription/requests/{id}/cancel` (admin) — withdraw a request still waiting for review.
pub async fn cancel(State(state): State<AppState>, admin: AuthUser, Path(id): Path<Uuid>) -> AppResult<Json<SchoolRequest>> {
    let mut tx = state.db.begin().await?;
    let status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM subscription_requests WHERE id = $1 AND school_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(admin.school_id)
    .fetch_optional(&mut *tx)
    .await?;
    match status.as_deref() {
        None => return Err(AppError::NotFound("Payment request not found.".into())),
        Some("pending") => {}
        Some(_) => {
            return Err(AppError::Conflict {
                message: "This request has already been reviewed and can't be cancelled.".into(),
                code: "already_reviewed",
                details: None,
            })
        }
    }
    sqlx::query("UPDATE subscription_requests SET status = 'cancelled', reviewed_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    activity::log(&mut tx, &admin, Action::Update, TABLE, id).await?;
    tx.commit().await?;
    Ok(Json(school_request(&state.db, admin.school_id, id).await?))
}

// ---------- Superadmin ----------

#[derive(Deserialize)]
pub struct ListQuery {
    /// `pending` (default), `approved`, `rejected`, `cancelled` or `all`.
    pub status: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct ReviewItem {
    pub id: Uuid,
    pub school_id: Uuid,
    pub school_name: String,
    pub requested_by_name: String,
    pub requested_by_username: String,
    pub plan: Plan,
    pub term: Term,
    pub amount_php: i32,
    pub payment_method: String,
    pub reference_no: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub review_note: Option<String>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
}

/// `GET /superadmin/requests?status=` — oldest pending first; reviewed ones newest first.
pub async fn review_list(
    State(state): State<AppState>,
    _sa: SuperadminAuth,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Vec<ReviewItem>>> {
    let status = match q.status.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Some("pending".to_string()),
        Some("all") => None,
        Some(s) if STATUSES.contains(&s) => Some(s.to_string()),
        Some(_) => return Err(AppError::BadRequest("Unknown status.".into())),
    };
    let rows = sqlx::query_as(
        "SELECT r.id, r.school_id, sc.name AS school_name,
                u.full_name AS requested_by_name, u.username AS requested_by_username,
                r.plan, r.term, r.amount_php, r.payment_method, r.reference_no, r.status,
                r.created_at, r.reviewed_at, r.review_note, s.start_date, s.end_date
           FROM subscription_requests r
           JOIN schools sc ON sc.id = r.school_id
           JOIN users u ON u.id = r.requested_by
           LEFT JOIN subscriptions s ON s.id = r.subscription_id
          WHERE ($1::text IS NULL OR r.status = $1)
          ORDER BY CASE WHEN r.status = 'pending' THEN r.created_at END ASC,
                   r.created_at DESC
          LIMIT 200",
    )
    .bind(&status)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Serialize)]
pub struct ReviewDetail {
    #[serde(flatten)]
    pub request: ReviewItem,
    /// `data:image/...;base64,...`
    pub receipt_image: String,
    /// The school's plan right now, to see what approving will do.
    pub current_subscription: SubscriptionInfo,
    /// Times this reference number appears in other requests (any status).
    pub reference_uses_elsewhere: i64,
}

async fn review_item(db: &sqlx::PgPool, id: Uuid) -> AppResult<ReviewItem> {
    sqlx::query_as(
        "SELECT r.id, r.school_id, sc.name AS school_name,
                u.full_name AS requested_by_name, u.username AS requested_by_username,
                r.plan, r.term, r.amount_php, r.payment_method, r.reference_no, r.status,
                r.created_at, r.reviewed_at, r.review_note, s.start_date, s.end_date
           FROM subscription_requests r
           JOIN schools sc ON sc.id = r.school_id
           JOIN users u ON u.id = r.requested_by
           LEFT JOIN subscriptions s ON s.id = r.subscription_id
          WHERE r.id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::NotFound("Payment request not found.".into()))
}

/// `GET /superadmin/requests/{id}` — one request with its receipt image.
pub async fn review_get(State(state): State<AppState>, _sa: SuperadminAuth, Path(id): Path<Uuid>) -> AppResult<Json<ReviewDetail>> {
    let request = review_item(&state.db, id).await?;
    let (image, mime): (Vec<u8>, String) =
        sqlx::query_as("SELECT receipt_image, receipt_mime FROM subscription_requests WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    let reference_uses_elsewhere: i64 =
        sqlx::query_scalar("SELECT count(*) FROM subscription_requests WHERE reference_no = $1 AND id <> $2")
            .bind(&request.reference_no)
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    let current_subscription = subscription::current(&state.db, request.school_id).await?;
    Ok(Json(ReviewDetail {
        receipt_image: data_url(&mime, &image),
        current_subscription,
        reference_uses_elsewhere,
        request,
    }))
}

#[derive(Deserialize, Default)]
pub struct ReviewInput {
    #[serde(default)]
    pub note: Option<String>,
}

fn clean_note(note: Option<&str>) -> AppResult<Option<String>> {
    let n = note.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
    if n.as_ref().is_some_and(|n| n.chars().count() > 500) {
        let mut e = FieldErrors::default();
        e.add("note", "Keep the note to 500 characters or fewer.");
        e.finish()?;
    }
    Ok(n)
}

/// Locks the school, then the request, and checks it's still pending.
/// Returns `(school_id, plan, term)`.
async fn lock_pending(tx: &mut sqlx::PgConnection, id: Uuid) -> AppResult<(Uuid, Plan, Term)> {
    let school_id: Uuid = sqlx::query_scalar("SELECT school_id FROM subscription_requests WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Payment request not found.".into()))?;
    sqlx::query("SELECT 1 FROM schools WHERE id = $1 FOR UPDATE")
        .bind(school_id)
        .execute(&mut *tx)
        .await?;
    let (status, plan, term): (String, Plan, Term) =
        sqlx::query_as("SELECT status, plan, term FROM subscription_requests WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    if status != "pending" {
        let message = match status.as_str() {
            "cancelled" => "The school cancelled this request.",
            _ => "This request has already been reviewed.",
        };
        return Err(AppError::Conflict { message: message.into(), code: "already_reviewed", details: None });
    }
    Ok((school_id, plan, term))
}

/// `POST /superadmin/requests/{id}/approve` `{ note? }` — the payment matched: start the plan.
pub async fn approve(
    State(state): State<AppState>,
    _sa: SuperadminAuth,
    Path(id): Path<Uuid>,
    body: axum::body::Bytes,
) -> AppResult<Json<ReviewItem>> {
    // The body is optional (`{ "note": "..." }`); an empty one is fine.
    let input: ReviewInput = if body.iter().all(u8::is_ascii_whitespace) {
        ReviewInput::default()
    } else {
        serde_json::from_slice(&body).map_err(|_| AppError::BadRequest("Invalid request body.".into()))?
    };
    let note = clean_note(input.note.as_deref())?;
    let mut tx = state.db.begin().await?;
    let (school_id, plan, term) = lock_pending(&mut tx, id).await?;
    let applied = subscription::apply_plan(&mut tx, &state.db, school_id, plan, term).await?;
    sqlx::query(
        "UPDATE subscription_requests
            SET status = 'approved', reviewed_at = now(), review_note = $2, subscription_id = $3
          WHERE id = $1",
    )
    .bind(id)
    .bind(&note)
    .bind(applied.subscription_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    tracing::info!(request_id = %id, %school_id, kind = applied.kind, end = %applied.end_date, "payment request approved");
    Ok(Json(review_item(&state.db, id).await?))
}

/// `POST /superadmin/requests/{id}/reject` `{ note }` — the payment didn't match. The reason is shown to the school.
pub async fn reject(
    State(state): State<AppState>,
    _sa: SuperadminAuth,
    Path(id): Path<Uuid>,
    Json(input): Json<ReviewInput>,
) -> AppResult<Json<ReviewItem>> {
    let note = clean_note(input.note.as_deref())?;
    let Some(note) = note else {
        let mut e = FieldErrors::default();
        e.add("note", "Tell the school why, e.g. \"No payment received with this reference number.\"");
        return Err(e.finish().unwrap_err());
    };
    let mut tx = state.db.begin().await?;
    lock_pending(&mut tx, id).await?;
    sqlx::query("UPDATE subscription_requests SET status = 'rejected', reviewed_at = now(), review_note = $2 WHERE id = $1")
        .bind(id)
        .bind(&note)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    tracing::info!(request_id = %id, "payment request rejected");
    Ok(Json(review_item(&state.db, id).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_normalized() {
        assert_eq!(normalize_reference(" 1012 345-678 901 "), "1012345678901");
        assert_eq!(normalize_reference("ab12cd"), "AB12CD");
        assert!(valid_reference("1012345678901"));
        assert!(!valid_reference("12345"));
        assert!(!valid_reference("1234#5678"));
    }

    #[test]
    fn receipts_must_be_real_images() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.resize(2048, 0);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        assert_eq!(decode_receipt(&format!("data:image/png;base64,{b64}")).unwrap().1, "image/png");
        assert_eq!(decode_receipt(&b64).unwrap().1, "image/png");
        // Declared type is ignored; content decides.
        let text = base64::engine::general_purpose::STANDARD.encode(vec![b'a'; 2048]);
        assert!(decode_receipt(&format!("data:image/png;base64,{text}")).is_err());
        assert!(decode_receipt("").is_err());
        assert!(decode_receipt("data:image/png;base64,!!!").is_err());
        let tiny = base64::engine::general_purpose::STANDARD.encode(b"\xFF\xD8\xFF\xE0tiny");
        assert!(decode_receipt(&tiny).is_err());
    }
}
