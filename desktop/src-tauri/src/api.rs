//! HTTP client for the Clinilog server. All server traffic goes through here,
//! so the access token never reaches the webview.

use chrono::{DateTime, NaiveDate, Utc};
use reqwest::{Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// Error returned to the frontend. `kind` drives UI behaviour:
/// - `network`: server unreachable or asleep (show "Connecting to server…" / retry)
/// - `unauthorized`: session missing or expired (go back to login)
/// - `bad_credentials`, `locked`, `forbidden`: show `message` to the user
/// - `invalid`: 400; `fields` maps field name -> message for inline errors
/// - `not_found`: 404
/// - `conflict`: 409; `code` says which (e.g. `possible_duplicate`), `details` has extra data
/// - `config`: this install isn't set up (server URL / school ID)
/// - `server`: unexpected server or app error
#[derive(Debug, Serialize)]
pub struct CmdError {
    pub kind: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl CmdError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), retry_after_secs: None, fields: None, code: None, details: None }
    }
    pub fn network() -> Self {
        Self::new("network", "Can't reach the Clinilog server. Check the internet connection.")
    }
    pub fn unauthorized() -> Self {
        Self::new("unauthorized", "Your session has ended. Please log in again.")
    }
}

#[derive(Deserialize)]
struct ServerError {
    error: String,
    retry_after_secs: Option<i64>,
    fields: Option<serde_json::Map<String, serde_json::Value>>,
    code: Option<String>,
    details: Option<serde_json::Value>,
}

// ---- Shapes returned by the server (mirrors /server) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: Uuid,
    pub full_name: String,
    pub username: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchoolInfo {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub user: UserInfo,
    pub school: SchoolInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limits {
    pub max_students: Option<i64>,
    pub max_users: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Features {
    pub advanced_search: bool,
    pub detailed_reports: bool,
    pub daily_backups: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionInfo {
    pub plan: String,
    pub status: String,
    /// The free trial a new school starts with. Missing in plans cached by older versions.
    #[serde(default)]
    pub is_trial: bool,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub days_left: Option<i64>,
    pub read_only: bool,
    pub limits: Limits,
    pub features: Features,
    /// Missing in plans cached by older app versions, hence the default.
    #[serde(default)]
    pub usage: Usage,
    pub server_date: NaiveDate,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub students: i64,
    pub active_users: i64,
}

#[derive(Clone)]
pub struct Api {
    http: reqwest::Client,
    base: url::Url,
}

impl Api {
    pub fn new(base: url::Url) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            // Render's free tier can take ~30–60 s to wake up.
            .timeout(Duration::from_secs(60))
            .user_agent(concat!("Clinilog-Desktop/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client builds");
        Self { http, base }
    }

    fn url(&self, path: &str) -> Result<url::Url, CmdError> {
        self.base
            .join(path)
            .map_err(|_| CmdError::new("config", "Server URL is not valid."))
    }

    /// `code: "offline"` = no connection at all (the server's name didn't resolve or
    /// nothing answered), so there's no point waiting; otherwise the server may be waking up.
    pub async fn health(&self) -> Result<(), CmdError> {
        let res = self
            .http
            .get(self.url("health")?)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|e| if e.is_connect() { offline() } else { CmdError::network() })?;
        if res.status().is_success() {
            Ok(())
        } else {
            Err(CmdError::new("network", "The server is starting up or having trouble."))
        }
    }

    /// Short health check for the background sync loop (doesn't wait for a sleeping server).
    pub async fn health_quick(&self) -> Result<(), CmdError> {
        let res = self
            .http
            .get(self.url("health")?)
            .timeout(Duration::from_secs(8))
            .send()
            .await
            .map_err(|_| CmdError::network())?;
        if res.status().is_success() {
            Ok(())
        } else {
            Err(CmdError::network())
        }
    }

    /// Finds a school by its code (no login needed).
    pub async fn lookup_school(&self, code: &str) -> Result<crate::config::SchoolRef, CmdError> {
        #[derive(Deserialize)]
        struct Found {
            id: Uuid,
            name: String,
            code: String,
        }
        let mut url = self.url("schools/lookup")?;
        url.query_pairs_mut().append_pair("code", code.trim());
        let res = self.http.get(url).timeout(Duration::from_secs(60)).send().await.map_err(|_| CmdError::network())?;
        match res.status() {
            s if s.is_success() => {
                let f: Found = res.json().await.map_err(|_| bad_response())?;
                Ok(crate::config::SchoolRef { id: f.id, name: Some(f.name), code: Some(f.code) })
            }
            other => Err(map_status(other, res).await),
        }
    }

    /// `school_id` is `None` only for the superadmin, who isn't part of a school.
    pub async fn login(&self, school_id: Option<Uuid>, username: &str, password: &str) -> Result<LoginResponse, CmdError> {
        let body = serde_json::json!({ "school_id": school_id, "username": username, "password": password });
        let res = self
            .http
            .post(self.url("auth/login")?)
            .json(&body)
            .send()
            .await
            .map_err(|_| CmdError::network())?;
        match res.status() {
            s if s.is_success() => res.json().await.map_err(|_| bad_response()),
            StatusCode::UNAUTHORIZED => Err(server_error(res, "bad_credentials").await),
            other => Err(map_status(other, res).await),
        }
    }

    pub async fn get_authed<T: DeserializeOwned>(&self, token: &str, path: &str) -> Result<T, CmdError> {
        let res = self
            .http
            .request(Method::GET, self.url(path)?)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| CmdError::network())?;
        match res.status() {
            s if s.is_success() => res.json().await.map_err(|_| bad_response()),
            // Keep the server's reason (e.g. "Your account is no longer active").
            StatusCode::UNAUTHORIZED => Err(server_error(res, "unauthorized").await),
            other => Err(map_status(other, res).await),
        }
    }
}

impl Api {
    /// Authenticated JSON request for the generic `api_request` command.
    /// Returns `None` for an empty (204) response.
    pub async fn request(
        &self,
        token: &str,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<Option<serde_json::Value>, CmdError> {
        let mut req = self.http.request(method, self.url(path)?).bearer_auth(token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let res = req.send().await.map_err(|_| CmdError::network())?;
        match res.status() {
            StatusCode::NO_CONTENT => Ok(None),
            s if s.is_success() => res.json().await.map(Some).map_err(|_| bad_response()),
            // Keep the server's reason (e.g. "Your account is no longer active").
            StatusCode::UNAUTHORIZED => Err(server_error(res, "unauthorized").await),
            other => Err(map_status(other, res).await),
        }
    }
}

/// Only plain relative API paths (`students/…?q=…`) may be requested,
/// so the token can never be sent to another host.
pub fn safe_api_path(path: &str) -> Result<&str, CmdError> {
    let p = path.strip_prefix('/').unwrap_or(path);
    let bad = p.is_empty()
        || p.contains("://")
        || p.contains("..")
        || p.contains('\\')
        || p.starts_with('/')
        || p.chars().any(|c| c.is_control() || c.is_whitespace());
    if bad {
        return Err(CmdError::new("invalid", "Invalid request path."));
    }
    Ok(p)
}

fn offline() -> CmdError {
    CmdError { code: Some("offline".into()), ..CmdError::new("network", "You're offline. Check the internet connection.") }
}

fn bad_response() -> CmdError {
    CmdError::new("server", "The server sent a response the app didn't understand.")
}

async fn server_error(res: reqwest::Response, kind: &'static str) -> CmdError {
    match res.json::<ServerError>().await {
        Ok(e) => CmdError {
            kind,
            message: e.error,
            retry_after_secs: e.retry_after_secs,
            fields: e.fields,
            code: e.code,
            details: e.details,
        },
        Err(_) => CmdError::new(kind, "The server returned an error."),
    }
}

async fn map_status(status: StatusCode, res: reqwest::Response) -> CmdError {
    let kind = match status {
        StatusCode::TOO_MANY_REQUESTS => "locked",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::CONFLICT => "conflict",
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => "invalid",
        // Render returns 502/503 while the service is waking up.
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT => {
            return CmdError::new("network", "The server is starting up. Try again in a moment.")
        }
        _ => "server",
    };
    server_error(res, kind).await
}

#[cfg(test)]
mod subscription_tests {
    use super::SubscriptionInfo;
    use serde_json::json;

    fn server_json() -> serde_json::Value {
        json!({
            "plan": "basic", "status": "active", "is_trial": true, "start_date": "2026-01-01", "end_date": "2026-12-31",
            "days_left": 40, "read_only": false,
            "limits": { "max_students": 500, "max_users": 2 },
            "features": { "advanced_search": false, "detailed_reports": false, "daily_backups": false },
            "usage": { "students": 12, "active_users": 2 },
            "server_date": "2026-11-20"
        })
    }

    #[test]
    fn every_server_field_reaches_the_webview() {
        // The webview reads this struct re-serialized, so a missing field is silently dropped.
        let info: SubscriptionInfo = serde_json::from_value(server_json()).unwrap();
        assert_eq!(serde_json::to_value(&info).unwrap(), server_json());
    }

    #[test]
    fn plans_cached_before_usage_existed_still_load() {
        let mut old = server_json();
        old.as_object_mut().unwrap().remove("usage");
        let info: SubscriptionInfo = serde_json::from_value(old).unwrap();
        assert_eq!((info.usage.students, info.usage.active_users), (0, 0));
    }
}

#[cfg(test)]
mod tests {
    use super::safe_api_path;

    #[test]
    fn allows_relative_api_paths() {
        assert_eq!(safe_api_path("students").unwrap(), "students");
        assert_eq!(safe_api_path("/students/abc").unwrap(), "students/abc");
        assert_eq!(safe_api_path("students?q=dela%20cruz&page=2").unwrap(), "students?q=dela%20cruz&page=2");
    }

    #[test]
    fn rejects_paths_that_could_leave_the_api() {
        for bad in ["", "/", "https://evil.example/x", "//evil.example/x", "///x", "../admin", "a/../../b", "a\\b", "a b", "a\nb"] {
            assert!(safe_api_path(bad).is_err(), "{bad:?} should be rejected");
        }
    }
}
