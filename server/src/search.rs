//! Turning search box text into safe SQL parameters (never into SQL text).

use chrono::NaiveDate;

use crate::error::{AppError, AppResult};

/// Max characters of search text that are looked at.
const MAX_QUERY_CHARS: usize = 100;
/// Max separate keywords in a complaint search.
const MAX_KEYWORDS: usize = 10;

/// Escapes LIKE wildcards so user input is matched literally (used with `ESCAPE '\'`).
pub fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn words(q: Option<&str>) -> Vec<String> {
    q.unwrap_or("")
        .chars()
        .take(MAX_QUERY_CHARS)
        .collect::<String>()
        .replace(',', " ")
        .split_whitespace()
        .map(escape_like)
        .collect()
}

/// Parameters for "name or LRN" search.
pub struct NameTerms {
    /// `%dela%cruz%juan%` — the words in order with anything between, for ILIKE.
    pub name_pattern: Option<String>,
    /// `1234%` when the query is a single run of digits, for an LRN prefix match.
    pub lrn_prefix: Option<String>,
}

pub fn name_terms(q: Option<&str>) -> NameTerms {
    let w = words(q);
    NameTerms {
        name_pattern: (!w.is_empty()).then(|| format!("%{}%", w.join("%"))),
        lrn_prefix: match w.as_slice() {
            [one] if one.chars().all(|c| c.is_ascii_digit()) => Some(format!("{one}%")),
            _ => None,
        },
    }
}

/// Complaint keywords: every word must appear, in any order. `None` when empty.
pub fn keywords(q: Option<&str>) -> Option<Vec<String>> {
    let mut w = words(q);
    w.truncate(MAX_KEYWORDS);
    (!w.is_empty()).then_some(w)
}

/// Parses an optional `YYYY-MM-DD` query value.
pub fn date_param(label: &str, value: Option<&str>) -> AppResult<Option<NaiveDate>> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) => NaiveDate::parse_from_str(v, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| AppError::BadRequest(format!("{label} must be a date in YYYY-MM-DD format."))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_terms_handle_order_commas_and_wildcards() {
        let t = name_terms(Some("  Dela Cruz,  Juan "));
        assert_eq!(t.name_pattern.as_deref(), Some("%Dela%Cruz%Juan%"));
        assert!(t.lrn_prefix.is_none());
        let t = name_terms(Some("1234"));
        assert_eq!(t.lrn_prefix.as_deref(), Some("1234%"));
        let t = name_terms(Some("50%_off\\"));
        assert_eq!(t.name_pattern.as_deref(), Some("%50\\%\\_off\\\\%"));
        assert!(name_terms(Some("   ")).name_pattern.is_none());
        assert!(name_terms(None).name_pattern.is_none());
    }

    #[test]
    fn keywords_are_capped_and_escaped() {
        assert_eq!(keywords(Some("head ache")).unwrap(), vec!["head", "ache"]);
        assert_eq!(keywords(Some("a b c d e f g h i j k l")).unwrap().len(), MAX_KEYWORDS);
        assert_eq!(keywords(Some("100%")).unwrap(), vec!["100\\%"]);
        assert!(keywords(Some("  ")).is_none());
    }

    #[test]
    fn date_param_parses_or_explains() {
        assert_eq!(date_param("From", Some("2026-11-20")).unwrap(), NaiveDate::from_ymd_opt(2026, 11, 20));
        assert_eq!(date_param("From", Some("")).unwrap(), None);
        assert!(date_param("From", Some("20/11/2026")).is_err());
    }
}
