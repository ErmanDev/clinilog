//! Seeds a fresh database. See `src/seed.rs` for what it creates.
//!
//! Usage:  SEED_ADMIN_PASSWORD=... cargo run --bin seed
//! (The server also does this by itself on startup when SEED_ADMIN_PASSWORD is set.)

use anyhow::{Context, Result};
use clinilog_server::{config, db, seed};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let (username, password) = seed::admin_from_env()
        .context("SEED_ADMIN_PASSWORD must be set (min 8 chars, with a letter and a number)")?;

    let pool = db::connect_and_migrate(&config::database_url_from_env()?).await?;
    let out = seed::run(&pool, &username, &password).await?;
    if out.created {
        println!("Seeded {} (id {})", seed::SCHOOL_NAME, out.school_id);
        println!("  admin user: {}", out.username);
    } else {
        println!("School already seeded (id {}); nothing to do.", out.school_id);
    }
    Ok(())
}
