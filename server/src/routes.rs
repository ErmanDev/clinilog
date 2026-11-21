use axum::{
    extract::State,
    http::StatusCode,
    middleware,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde_json::{json, Value};

use crate::{
    activity,
    auth::{self, require_admin, require_auth},
    medicines, reports, sections, students, subscription, users, visits, AppState,
};

/// Route groups:
/// - public:    no token needed
/// - signed-in: any active user (admin or staff); handlers scope queries by `user.school_id`
/// - admin:     signed-in AND role = admin
pub fn router(state: AppState) -> Router<AppState> {
    let public = Router::new()
        .route("/health", get(health))
        .route("/auth/login", post(auth::handlers::login));

    let admin = Router::new()
        .route("/users", get(users::list).post(users::create))
        .route("/users/{id}", put(users::update))
        .route("/users/{id}/disable", post(users::disable))
        .route("/users/{id}/enable", post(users::enable))
        .route("/users/{id}/reset-password", post(users::reset_password))
        .route("/activity", get(activity::list))
        .route("/sections", post(sections::create))
        .route("/sections/{id}", put(sections::update).delete(sections::delete))
        .route("/students/{id}", delete(students::delete))
        .route("/medicines", post(medicines::create))
        .route("/medicines/{id}", put(medicines::update).delete(medicines::delete))
        .route("/visits/{id}", delete(visits::delete))
        .route_layer(middleware::from_fn(require_admin));

    let signed_in = Router::new()
        .route("/auth/me", get(auth::handlers::me))
        .route("/subscription", get(subscription::get))
        .route("/sections", get(sections::list))
        .route("/students", get(students::list).post(students::create))
        .route("/students/lrn-check", get(students::lrn_check))
        .route("/students/{id}", get(students::get).put(students::update))
        .route("/medicines", get(medicines::list))
        .route("/visits", get(visits::list).post(visits::create))
        .route("/visits/{id}", get(visits::get).put(visits::update))
        .route("/dashboard", get(visits::dashboard))
        .route("/reports/basic", get(reports::basic))
        .route("/reports/detailed", get(reports::detailed))
        .merge(admin)
        .route_layer(middleware::from_fn_with_state(state, require_auth));

    public.merge(signed_in)
}

/// Liveness + database check. The desktop app can call this to show
/// "Connecting to server…" while Render's free tier wakes up.
async fn health(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    match sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&state.db).await {
        Ok(_) => (StatusCode::OK, Json(json!({ "status": "ok", "database": "ok" }))),
        Err(e) => {
            tracing::error!(error = %e, "health check: database unreachable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "degraded", "database": "unreachable" })),
            )
        }
    }
}
