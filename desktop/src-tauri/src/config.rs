//! Where the server is and which school this install belongs to.
//!
//! Values come from, in order:
//! 1. `config.json` in the app's config folder
//!    (Windows: `%APPDATA%\com.clinilog.desktop\config.json`), if present:
//!    `{ "api_url": "https://…", "school_id": "uuid", "school_name": "…", "school_code": "VNHS" }`
//!    — any key may be omitted. The app writes the school keys when someone enters a school code.
//! 2. Build-time env vars `CLINILOG_API_URL` and `CLINILOG_SCHOOL_ID`. A build with
//!    `CLINILOG_SCHOOL_ID` is locked to that school; without it, the app asks for a school code.
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
    pub school: Option<SchoolRef>,
    /// The school was built into this installer and can't be changed.
    pub school_locked: bool,
}

/// The school this copy of the app is connected to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchoolRef {
    pub id: Uuid,
    /// Unknown for a school built into the installer (shown after login instead).
    pub name: Option<String>,
    pub code: Option<String>,
}

#[derive(Deserialize, Default)]
struct FileConfig {
    api_url: Option<String>,
    school_id: Option<String>,
    school_name: Option<String>,
    school_code: Option<String>,
}

/// Safe-to-show summary for the UI.
#[derive(Serialize)]
pub struct ConfigInfo {
    pub api_url: String,
    pub school_configured: bool,
    pub school: Option<SchoolRef>,
    pub school_locked: bool,
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

    let parse = |s: &str| Uuid::parse_str(s.trim()).map_err(|_| format!("School ID is not valid: {s}"));
    let built_in = BUILD_SCHOOL_ID.map(parse).transpose()?;
    let school = match (built_in, file.school_id) {
        // An installer made for one school always uses it.
        (Some(id), _) => Some(SchoolRef { id, name: None, code: None }),
        (None, Some(s)) => Some(SchoolRef { id: parse(&s)?, name: file.school_name, code: file.school_code }),
        (None, None) => None,
    };

    Ok(AppConfig { api_url, school, school_locked: built_in.is_some() })
}

/// Saves (or with `None`, forgets) the school in `config.json`, keeping any other keys.
pub fn save_school(config_dir: &Path, school: Option<&SchoolRef>) -> Result<(), String> {
    let path = config_dir.join("config.json");
    let mut map = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&s).ok())
        .unwrap_or_default();
    for key in ["school_id", "school_name", "school_code"] {
        map.remove(key);
    }
    if let Some(s) = school {
        map.insert("school_id".into(), s.id.to_string().into());
        if let Some(n) = &s.name {
            map.insert("school_name".into(), n.clone().into());
        }
        if let Some(c) = &s.code {
            map.insert("school_code".into(), c.clone().into());
        }
    }
    std::fs::create_dir_all(config_dir).map_err(|e| format!("Couldn't save the school: {e}"))?;
    let text = serde_json::to_string_pretty(&map).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("Couldn't save the school: {e}"))
}
