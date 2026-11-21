//! Where the server is and which school this install belongs to.
//!
//! Values come from, in order:
//! 1. `config.json` in the app's config folder
//!    (Windows: `%APPDATA%\com.clinilog.desktop\config.json`), if present:
//!    `{ "api_url": "https://…", "school_id": "uuid" }` — either key may be omitted.
//! 2. Build-time env vars `CLINILOG_API_URL` and `CLINILOG_SCHOOL_ID`.
//! 3. Dev default `http://localhost:8080` for the URL (debug builds only).
//!
//! Release builds refuse a non-HTTPS server URL.

use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

const BUILD_API_URL: Option<&str> = option_env!("CLINILOG_API_URL");
const BUILD_SCHOOL_ID: Option<&str> = option_env!("CLINILOG_SCHOOL_ID");

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub api_url: url::Url,
    pub school_id: Option<Uuid>,
}

#[derive(Deserialize, Default)]
struct FileConfig {
    api_url: Option<String>,
    school_id: Option<String>,
}

/// Safe-to-show summary for the UI.
#[derive(Serialize)]
pub struct ConfigInfo {
    pub api_url: String,
    pub school_configured: bool,
    pub problem: Option<String>,
    pub version: String,
}

pub fn load(config_dir: Option<&Path>) -> Result<AppConfig, String> {
    let file: FileConfig = config_dir
        .map(|d| d.join("config.json"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| serde_json::from_str(&s).map_err(|e| format!("config.json is not valid: {e}")))
        .transpose()?
        .unwrap_or_default();

    let raw_url = file
        .api_url
        .or_else(|| BUILD_API_URL.map(str::to_string))
        .or_else(|| cfg!(debug_assertions).then(|| "http://localhost:8080".to_string()))
        .ok_or("No server URL is configured for this installation.")?;
    let api_url = url::Url::parse(raw_url.trim()).map_err(|_| format!("Server URL is not valid: {raw_url}"))?;
    if !cfg!(debug_assertions) && api_url.scheme() != "https" {
        return Err("The server URL must start with https://".into());
    }

    let school_id = file
        .school_id
        .or_else(|| BUILD_SCHOOL_ID.map(str::to_string))
        .map(|s| Uuid::parse_str(s.trim()).map_err(|_| format!("School ID is not valid: {s}")))
        .transpose()?;

    Ok(AppConfig { api_url, school_id })
}
