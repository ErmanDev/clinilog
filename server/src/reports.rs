//! Reports. Basic (every plan): visits per month and most common complaints.
//! Detailed (Premium): visits by grade/section, medicine usage, frequent visitors.
//!
//! A report covers a Philippine calendar year, or one month of it.

use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{AppError, AppResult},
    subscription::current,
    validate::today_ph,
    visits::ph_day_bounds,
    AppState,
};

const TOP_COMPLAINTS: usize = 10;
/// A complaint term must appear in at least this many visits to be listed.
const MIN_COMPLAINT_VISITS: i64 = 2;
const TOP_VISITORS: i64 = 10;

// ---------- Period ----------

#[derive(Deserialize)]
pub struct PeriodQuery {
    pub year: Option<i32>,
    /// 1–12; omitted = the whole year.
    pub month: Option<u32>,
}

#[derive(Serialize, Clone, Copy)]
pub struct Period {
    pub year: i32,
    pub month: Option<u32>,
    /// First and last Philippine calendar day covered (inclusive).
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl Period {
    fn from_query(q: &PeriodQuery) -> AppResult<Self> {
        let this_year = today_ph().year();
        let year = q.year.unwrap_or(this_year);
        if !(2000..=this_year + 1).contains(&year) {
            return Err(AppError::BadRequest(format!("Year must be from 2000 to {}.", this_year + 1)));
        }
        let (start, end) = match q.month {
            None => (ymd(year, 1, 1), ymd(year, 12, 31)),
            Some(m) if (1..=12).contains(&m) => {
                let first = ymd(year, m, 1);
                let next = if m == 12 { ymd(year + 1, 1, 1) } else { ymd(year, m + 1, 1) };
                (first, next.pred_opt().expect("valid date"))
            }
            Some(_) => return Err(AppError::BadRequest("Month must be from 1 to 12.".into())),
        };
        Ok(Self { year, month: q.month, start, end })
    }

    /// UTC instants bounding the period's Philippine days.
    fn bounds(&self) -> (DateTime<Utc>, DateTime<Utc>) {
        (ph_day_bounds(self.start).0, ph_day_bounds(self.end).1)
    }

    fn year_bounds(&self) -> (DateTime<Utc>, DateTime<Utc>) {
        (ph_day_bounds(ymd(self.year, 1, 1)).0, ph_day_bounds(ymd(self.year, 12, 31)).1)
    }
}

fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
}

// ---------- Complaint terms ----------

/// Filler words that don't describe a complaint (English and Filipino).
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "the", "of", "on", "in", "at", "to", "for", "from", "with", "without", "since", "after",
    "before", "during", "while", "when", "then", "than", "is", "was", "are", "were", "be", "been", "has", "had",
    "have", "my", "his", "her", "their", "he", "she", "they", "it", "its", "this", "that", "very", "mild",
    "slight", "slightly", "severe", "bit", "some", "also", "still", "not", "no", "yes", "but", "or", "due",
    "about", "around", "today", "yesterday", "morning", "afternoon", "class", "recess", "lunch", "first",
    "second", "third", "ng", "sa", "ang", "na", "mga", "si", "ni", "ay", "po", "lang", "din", "rin", "kasi",
    "pa", "nang", "may", "siya", "ako", "kanina", "noong",
];

/// Words of a complaint: lower-case letters only (ñ and accents kept), at least 3 letters.
/// Filler words become `None`, which breaks up phrases.
fn tokens(text: &str) -> Vec<Option<String>> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .map(|w| (w.chars().count() >= 3 && !STOPWORDS.contains(&w)).then(|| w.to_string()))
        .collect()
}

#[derive(Serialize, Debug, PartialEq)]
pub struct TermCount {
    /// Lower-case word or two-word phrase, e.g. "headache" or "stomach ache".
    pub term: String,
    /// Visits whose complaint mentions it.
    pub visits: i64,
}

/// Most common complaint terms. Each term counts once per visit. A two-word phrase
/// seen in 2+ visits is kept, and it replaces a word whose every mention is in that phrase
/// (so "stomach ache" ×5 shows instead of "stomach" ×5 and "ache" ×5). Terms in fewer
/// than `min_visits` visits are left out (a word used once is context, not a trend).
pub fn top_terms<'a>(complaints: impl IntoIterator<Item = &'a str>, limit: usize, min_visits: i64) -> Vec<TermCount> {
    let mut words: HashMap<String, i64> = HashMap::new();
    let mut pairs: HashMap<String, i64> = HashMap::new();
    for c in complaints {
        let toks = tokens(c);
        let uniq: HashSet<&String> = toks.iter().flatten().collect();
        for w in uniq {
            *words.entry(w.clone()).or_default() += 1;
        }
        let uniq_pairs: HashSet<String> = toks
            .windows(2)
            .filter_map(|p| match (&p[0], &p[1]) {
                (Some(a), Some(b)) if a != b => Some(format!("{a} {b}")),
                _ => None,
            })
            .collect();
        for p in uniq_pairs {
            *pairs.entry(p).or_default() += 1;
        }
    }

    let kept_pairs: Vec<(String, i64)> = pairs.into_iter().filter(|(_, n)| *n >= 2).collect();
    let mut out: Vec<TermCount> = kept_pairs
        .iter()
        .map(|(p, n)| TermCount { term: p.clone(), visits: *n })
        .collect();
    for (w, n) in words {
        let covered = kept_pairs
            .iter()
            .any(|(p, pn)| *pn == n && p.split(' ').any(|part| part == w));
        if !covered {
            out.push(TermCount { term: w, visits: n });
        }
    }
    out.retain(|t| t.visits >= min_visits);
    out.sort_by(|a, b| b.visits.cmp(&a.visits).then_with(|| a.term.cmp(&b.term)));
    out.truncate(limit);
    out
}

// ---------- Basic report ----------

#[derive(Serialize)]
pub struct MonthCount {
    pub month: u32,
    pub visits: i64,
}

#[derive(Serialize)]
pub struct BasicReport {
    pub period: Period,
    /// Visits in the period, and how many different students they were.
    pub total_visits: i64,
    pub students_seen: i64,
    /// Always the whole year, January–December.
    pub visits_per_month: Vec<MonthCount>,
    pub top_complaints: Vec<TermCount>,
}

/// `GET /reports/basic?year=&month=`
pub async fn basic(State(state): State<AppState>, user: AuthUser, Query(q): Query<PeriodQuery>) -> AppResult<Json<BasicReport>> {
    let period = Period::from_query(&q)?;
    let (from, to) = period.bounds();
    let (yfrom, yto) = period.year_bounds();

    let (total_visits, students_seen): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(DISTINCT student_id) FROM clinic_visits
          WHERE school_id = $1 AND visited_at >= $2 AND visited_at < $3",
    )
    .bind(user.school_id)
    .bind(from)
    .bind(to)
    .fetch_one(&state.db)
    .await?;

    let monthly: Vec<(i32, i64)> = sqlx::query_as(
        "SELECT extract(month FROM visited_at AT TIME ZONE 'Asia/Manila')::int AS m, count(*)
           FROM clinic_visits
          WHERE school_id = $1 AND visited_at >= $2 AND visited_at < $3
          GROUP BY 1",
    )
    .bind(user.school_id)
    .bind(yfrom)
    .bind(yto)
    .fetch_all(&state.db)
    .await?;
    let by_month: BTreeMap<i32, i64> = monthly.into_iter().collect();
    let visits_per_month = (1..=12u32)
        .map(|m| MonthCount { month: m, visits: by_month.get(&(m as i32)).copied().unwrap_or(0) })
        .collect();

    let complaints: Vec<String> = sqlx::query_scalar(
        "SELECT complaint FROM clinic_visits WHERE school_id = $1 AND visited_at >= $2 AND visited_at < $3",
    )
    .bind(user.school_id)
    .bind(from)
    .bind(to)
    .fetch_all(&state.db)
    .await?;
    let top_complaints = top_terms(complaints.iter().map(String::as_str), TOP_COMPLAINTS, MIN_COMPLAINT_VISITS);

    Ok(Json(BasicReport { period, total_visits, students_seen, visits_per_month, top_complaints }))
}

// ---------- Detailed report (Premium) ----------

#[derive(Serialize, sqlx::FromRow)]
pub struct SectionCount {
    pub grade_level: i32,
    pub section_name: String,
    pub visits: i64,
    pub students: i64,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct MedicineUsage {
    pub name: String,
    pub unit: String,
    pub total_quantity: i64,
    pub visits: i64,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct FrequentVisitor {
    pub student_id: Uuid,
    pub lrn: String,
    pub first_name: String,
    pub middle_name: Option<String>,
    pub last_name: String,
    pub grade_level: i32,
    pub section_name: String,
    pub visits: i64,
    pub last_visit: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct DetailedReport {
    pub period: Period,
    pub by_section: Vec<SectionCount>,
    pub medicines: Vec<MedicineUsage>,
    pub frequent_visitors: Vec<FrequentVisitor>,
}

/// `GET /reports/detailed?year=&month=` (Premium)
pub async fn detailed(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<PeriodQuery>,
) -> AppResult<Json<DetailedReport>> {
    if !current(&state.db, user.school_id).await?.features.detailed_reports {
        return Err(AppError::PremiumRequired("Detailed reports are part of the Premium plan.".into()));
    }
    let period = Period::from_query(&q)?;
    let (from, to) = period.bounds();

    let by_section: Vec<SectionCount> = sqlx::query_as(
        "SELECT s.grade_level, s.section_name, count(*) AS visits, count(DISTINCT v.student_id) AS students
           FROM clinic_visits v
           JOIN students st ON st.id = v.student_id
           JOIN sections s ON s.id = st.section_id
          WHERE v.school_id = $1 AND v.visited_at >= $2 AND v.visited_at < $3
          GROUP BY s.grade_level, s.section_name
          ORDER BY s.grade_level, lower(s.section_name)",
    )
    .bind(user.school_id)
    .bind(from)
    .bind(to)
    .fetch_all(&state.db)
    .await?;

    let medicines: Vec<MedicineUsage> = sqlx::query_as(
        "SELECT m.name, m.unit, sum(vm.quantity)::bigint AS total_quantity, count(DISTINCT v.id) AS visits
           FROM visit_medicines vm
           JOIN clinic_visits v ON v.id = vm.visit_id
           JOIN medicines m ON m.id = vm.medicine_id
          WHERE v.school_id = $1 AND v.visited_at >= $2 AND v.visited_at < $3
          GROUP BY m.id, m.name, m.unit
          ORDER BY total_quantity DESC, lower(m.name)",
    )
    .bind(user.school_id)
    .bind(from)
    .bind(to)
    .fetch_all(&state.db)
    .await?;

    let frequent_visitors: Vec<FrequentVisitor> = sqlx::query_as(
        "SELECT st.id AS student_id, trim(st.lrn) AS lrn, st.first_name, st.middle_name, st.last_name,
                s.grade_level, s.section_name, count(*) AS visits, max(v.visited_at) AS last_visit
           FROM clinic_visits v
           JOIN students st ON st.id = v.student_id
           JOIN sections s ON s.id = st.section_id
          WHERE v.school_id = $1 AND v.visited_at >= $2 AND v.visited_at < $3
          GROUP BY st.id, s.grade_level, s.section_name
          ORDER BY visits DESC, max(v.visited_at) DESC
          LIMIT $4",
    )
    .bind(user.school_id)
    .bind(from)
    .bind(to)
    .bind(TOP_VISITORS)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(DetailedReport { period, by_section, medicines, frequent_visitors }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(list: &[&str]) -> Vec<(String, i64)> {
        top_terms(list.iter().copied(), 10, 1).into_iter().map(|t| (t.term, t.visits)).collect()
    }

    #[test]
    fn terms_below_the_minimum_are_left_out() {
        let t = top_terms(["Headache", "Headache during basketball", "Fever"], 10, 2);
        assert_eq!(t, vec![TermCount { term: "headache".into(), visits: 2 }]);
    }

    #[test]
    fn counts_each_term_once_per_visit_and_skips_filler() {
        let t = terms(&["Headache since morning", "Headache headache!!", "Fever and headache"]);
        assert_eq!(t[0], ("headache".into(), 3));
        assert!(t.contains(&("fever".into(), 1)));
        assert!(!t.iter().any(|(w, _)| w == "since" || w == "and" || w == "morning"));
    }

    #[test]
    fn phrase_replaces_words_it_fully_explains() {
        let t = terms(&["Stomach ache", "stomach ache after recess", "Ache in stomach"]);
        // "stomach ache" in 2 visits; "stomach" and "ache" each in 3 (the third visit
        // says "ache in stomach"), so the words stay too.
        assert!(t.contains(&("stomach ache".into(), 2)));
        assert!(t.contains(&("stomach".into(), 3)));
        let t = terms(&["Stomach ache", "stomach ache after recess"]);
        assert_eq!(t, vec![("stomach ache".into(), 2)]);
    }

    #[test]
    fn single_phrase_is_not_promoted_and_filler_breaks_phrases() {
        let t = terms(&["Sprained ankle", "Headache and dizziness", "Headache and dizziness"]);
        assert!(!t.iter().any(|(w, _)| w == "sprained ankle"), "seen once only");
        assert!(!t.iter().any(|(w, _)| w == "headache dizziness"), "'and' separates them");
        assert!(t.contains(&("headache".into(), 2)) && t.contains(&("dizziness".into(), 2)));
    }

    #[test]
    fn keeps_filipino_letters_and_orders_ties_alphabetically() {
        let t = terms(&["Masakit ang ulo", "Ubo at sipon", "Pagsusuka"]);
        assert_eq!(t.iter().map(|(w, _)| w.as_str()).collect::<Vec<_>>(), ["masakit", "pagsusuka", "sipon", "ubo", "ulo"]);
        assert!(terms(&["Niño's sakít"]).iter().any(|(w, _)| w == "sakít"));
    }

    #[test]
    fn period_bounds() {
        let p = Period::from_query(&PeriodQuery { year: Some(2024), month: Some(2) }).unwrap();
        assert_eq!((p.start, p.end), (ymd(2024, 2, 1), ymd(2024, 2, 29)));
        let p = Period::from_query(&PeriodQuery { year: Some(2025), month: Some(12) }).unwrap();
        assert_eq!(p.end, ymd(2025, 12, 31));
        let p = Period::from_query(&PeriodQuery { year: Some(2025), month: None }).unwrap();
        assert_eq!((p.start, p.end), (ymd(2025, 1, 1), ymd(2025, 12, 31)));
        assert!(Period::from_query(&PeriodQuery { year: Some(1999), month: None }).is_err());
        assert!(Period::from_query(&PeriodQuery { year: Some(2025), month: Some(13) }).is_err());
    }
}
