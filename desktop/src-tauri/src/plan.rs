//! Plan fetch with a 7-day offline grace period.
//!
//! Every successful fetch is saved to `plan_cache.json` in the app data folder.
//! If the server can't be reached, the last known plan is used for up to
//! 7 days after it was fetched; after that the app switches to read-only
//! until it reaches the server again. The cache holds no student data.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::api::SubscriptionInfo;

pub const GRACE_DAYS: i64 = 7;
const CACHE_FILE: &str = "plan_cache.json";

#[derive(Serialize, Deserialize)]
struct CacheFile {
    school_id: Uuid,
    fetched_at: DateTime<Utc>,
    info: SubscriptionInfo,
}

#[derive(Serialize)]
pub struct PlanState {
    pub info: SubscriptionInfo,
    /// `server` = fresh from the server; `cache` = last known plan (offline).
    pub source: &'static str,
    pub fetched_at: DateTime<Utc>,
    /// Only when `source == "cache"`: when the offline grace period ends.
    pub grace_ends_at: Option<DateTime<Utc>>,
    /// Offline for longer than the grace period; `info.read_only` is forced on.
    pub grace_expired: bool,
}

fn cache_path(dir: &Path) -> PathBuf {
    dir.join(CACHE_FILE)
}

pub fn save(dir: Option<&Path>, school_id: Uuid, info: &SubscriptionInfo) -> PlanState {
    let fetched_at = Utc::now();
    if let Some(dir) = dir {
        let file = CacheFile { school_id, fetched_at, info: info.clone() };
        // Best effort: a failed write only means no offline fallback next time.
        if std::fs::create_dir_all(dir).is_ok() {
            if let Ok(json) = serde_json::to_vec_pretty(&file) {
                let _ = std::fs::write(cache_path(dir), json);
            }
        }
    }
    PlanState { info: info.clone(), source: "server", fetched_at, grace_ends_at: None, grace_expired: false }
}

/// The last known plan for this school, with the grace rule applied.
pub fn load_cached(dir: Option<&Path>, school_id: Uuid) -> Option<PlanState> {
    let bytes = std::fs::read(cache_path(dir?)).ok()?;
    let file: CacheFile = serde_json::from_slice(&bytes).ok()?;
    if file.school_id != school_id {
        return None;
    }
    let grace_ends_at = file.fetched_at + Duration::days(GRACE_DAYS);
    let grace_expired = Utc::now() > grace_ends_at;
    let mut info = file.info;
    if grace_expired {
        info.read_only = true;
    }
    Some(PlanState {
        info,
        source: "cache",
        fetched_at: file.fetched_at,
        grace_ends_at: Some(grace_ends_at),
        grace_expired,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Features, Limits};

    fn info() -> SubscriptionInfo {
        SubscriptionInfo {
            plan: "basic".into(),
            status: "active".into(),
            start_date: None,
            end_date: None,
            days_left: Some(100),
            read_only: false,
            limits: Limits { max_students: Some(500), max_users: 2 },
            features: Features { advanced_search: false, detailed_reports: false, daily_backups: false },
            usage: Default::default(),
            server_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("clinilog-plan-test-{tag}-{nanos}"))
    }

    fn write_cache(dir: &Path, school_id: Uuid, age_days: i64) {
        std::fs::create_dir_all(dir).unwrap();
        let file = CacheFile { school_id, fetched_at: Utc::now() - Duration::days(age_days), info: info() };
        std::fs::write(cache_path(dir), serde_json::to_vec(&file).unwrap()).unwrap();
    }

    #[test]
    fn within_grace_keeps_last_known_plan() {
        let (dir, school) = (temp_dir("within"), Uuid::from_u128(1));
        write_cache(&dir, school, 6);
        let state = load_cached(Some(&dir), school).unwrap();
        assert_eq!(state.source, "cache");
        assert!(!state.grace_expired);
        assert!(!state.info.read_only);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn after_grace_forces_read_only() {
        let (dir, school) = (temp_dir("after"), Uuid::from_u128(2));
        write_cache(&dir, school, 8);
        let state = load_cached(Some(&dir), school).unwrap();
        assert!(state.grace_expired);
        assert!(state.info.read_only);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn ignores_another_schools_cache() {
        let dir = temp_dir("other");
        write_cache(&dir, Uuid::from_u128(3), 1);
        assert!(load_cached(Some(&dir), Uuid::from_u128(4)).is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn save_then_load_round_trips() {
        let (dir, school) = (temp_dir("save"), Uuid::from_u128(5));
        let saved = save(Some(&dir), school, &info());
        assert_eq!(saved.source, "server");
        let loaded = load_cached(Some(&dir), school).unwrap();
        assert_eq!(loaded.info.plan, "basic");
        assert!(!loaded.grace_expired);
        std::fs::remove_dir_all(dir).ok();
    }
}
