pub mod activity;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod medicines;
pub mod payments;
pub mod reports;
pub mod routes;
pub mod schools;
pub mod search;
pub mod seed;
pub mod sections;
pub mod students;
pub mod subscription;
pub mod users;
pub mod validate;
pub mod visits;

use axum::Router;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub jwt: auth::jwt::JwtKeys,
    /// `None` when no superadmin is configured (payment requests can't be reviewed).
    pub superadmin: Option<std::sync::Arc<auth::superadmin::Superadmin>>,
}

pub fn app(state: AppState) -> Router {
    routes::router(state.clone())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
