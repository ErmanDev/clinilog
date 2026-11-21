use anyhow::{bail, Context, Result};

/// Runtime configuration, read from environment variables (or a local `.env` file).
/// Secrets are never hard-coded; on Render they come from the service's env vars.
pub struct Config {
    pub database_url: String,
    pub port: u16,
    pub jwt_secret: String,
    pub jwt_ttl_minutes: i64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
        let port = match std::env::var("PORT") {
            Ok(p) => p.parse().context("PORT must be a valid port number")?,
            Err(_) => 8080,
        };
        let jwt_secret = std::env::var("JWT_SECRET").context("JWT_SECRET must be set")?;
        if jwt_secret.len() < 32 {
            bail!("JWT_SECRET must be at least 32 characters");
        }
        let jwt_ttl_minutes = match std::env::var("JWT_TTL_MINUTES") {
            Ok(v) => v.parse().context("JWT_TTL_MINUTES must be a whole number")?,
            Err(_) => 480,
        };
        if !(5..=24 * 60).contains(&jwt_ttl_minutes) {
            bail!("JWT_TTL_MINUTES must be between 5 and 1440");
        }
        Ok(Self { database_url, port, jwt_secret, jwt_ttl_minutes })
    }
}

/// Database-only config, for tools like the seed that don't need auth settings.
pub fn database_url_from_env() -> Result<String> {
    std::env::var("DATABASE_URL").context("DATABASE_URL must be set")
}
