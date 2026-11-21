use anyhow::Result;
use clinilog_server::{app, auth::jwt::JwtKeys, config::Config, db, seed, AppState};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("clinilog_server=info,tower_http=info")),
        )
        .init();

    let config = Config::from_env()?;
    let pool = db::connect_and_migrate(&config.database_url).await?;

    // First-time setup on hosts without a shell (e.g. Render's free tier).
    // Does nothing once the school exists.
    if let Some((username, password)) = seed::admin_from_env() {
        let out = seed::run(&pool, &username, &password).await?;
        if out.created {
            tracing::info!(school_id = %out.school_id, admin = %out.username, "seeded first school; use this school_id for the desktop build");
        } else {
            tracing::info!(school_id = %out.school_id, "school already set up; SEED_ADMIN_PASSWORD can be removed");
        }
    }

    // TLS is terminated by Render's proxy; the service itself listens on plain HTTP.
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Clinilog server listening on {addr}");

    let jwt = JwtKeys::new(&config.jwt_secret, config.jwt_ttl_minutes);
    axum::serve(listener, app(AppState { db: pool, jwt }))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutting down");
}
