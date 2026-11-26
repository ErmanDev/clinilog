//! Answers the app's API requests from the offline copy (`mirror.rs`) while the
//! server can't be reached, the same way the server would: same paths, same JSON
//! shapes, same field checks and messages, same permission rules.
//!
//! - `read` handles GETs of students, visits, sections, medicines, the dashboard and reports.
//! - `write` checks a change and returns the rows it changes plus what to upload later.
//!   Replaying a saved change (`Ctx::replay`) skips the checks that only matter when
//!   the change is first made (permissions, plan, duplicate warning, visit time).
//!
//! Keep in step with the server: `server/src/{students,visits,sections,medicines,reports,
//! search,validate}.rs`.

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, TimeZone, Utc};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

use crate::{
    api::CmdError,
    mirror::{Change, MLine, MMedicine, MSection, MStudent, MVisit, Mirror, Table},
};

const MAX_PAGE_SIZE: i64 = 100;
const MAX_QUERY_CHARS: usize = 100;
const MAX_KEYWORDS: usize = 10;
const MAX_MEDICINE_LINES: usize = 20;
const CLOCK_SKEW_MINUTES: i64 = 5;
const MAX_BACKDATE_DAYS: i64 = 30;
const NAME_MAX: usize = 50;
const MIN_AGE: i32 = 4;
const MAX_AGE: i32 = 25;

/// The plan flags that change what's allowed (from the last plan fetched from the server).
#[derive(Debug, Clone, Default)]
pub struct PlanFlags {
    pub advanced_search: bool,
    pub detailed_reports: bool,
    pub read_only: bool,
    /// Offline for longer than the grace period.
    pub grace_expired: bool,
    pub max_students: Option<i64>,
}

pub struct Ctx<'a> {
    pub user_id: Uuid,
    pub user_name: &'a str,
    pub is_admin: bool,
    /// `None` when no plan is known: treated as Basic and read-only, like the app does.
    pub plan: Option<&'a PlanFlags>,
    pub now: DateTime<Utc>,
    /// Re-applying a change saved earlier rather than making a new one.
    pub replay: bool,
}

impl Ctx<'_> {
    fn today(&self) -> NaiveDate {
        self.now.with_timezone(&ph()).date_naive()
    }
}

/// A GET answered from the offline copy.
#[derive(Debug)]
pub struct Answer {
    pub body: Value,
    /// A student or visit record was opened: logged as a view once back online.
    pub viewed: Option<(&'static str, Uuid)>,
}

/// A change made offline.
#[derive(Debug)]
pub struct Written {
    /// What the server would have answered; `None` for an empty (204) reply.
    pub response: Option<Value>,
    pub changes: Vec<Change>,
    /// What to upload later: the request path and body, with the new record's id
    /// (and a new visit's time) filled in so the upload is safe to retry.
    pub path: String,
    pub body: Option<Value>,
    /// For the "waiting to upload" list, e.g. "New visit" / "Dela Cruz, Juan" / "Headache".
    pub action: &'static str,
    pub label: String,
    pub summary: String,
}

// ---------- errors ----------

fn bad_request(msg: impl Into<String>) -> CmdError {
    CmdError::new("invalid", msg)
}

fn not_found(what: &str) -> CmdError {
    CmdError::new("not_found", format!("{what} not found."))
}

fn forbidden(msg: impl Into<String>) -> CmdError {
    CmdError::new("forbidden", msg)
}

fn coded(kind: &'static str, code: &str, msg: impl Into<String>, details: Option<Value>) -> CmdError {
    CmdError { code: Some(code.into()), details, ..CmdError::new(kind, msg) }
}

fn premium(msg: &str) -> CmdError {
    coded("forbidden", "premium_required", msg, None)
}

#[derive(Default)]
struct Fields(Map<String, Value>);

impl Fields {
    fn add(&mut self, field: impl Into<String>, msg: impl Into<String>) {
        self.0.entry(field.into()).or_insert_with(|| Value::String(msg.into()));
    }
    fn has(&self, field: &str) -> bool {
        self.0.contains_key(field)
    }
    fn finish(self) -> Result<(), CmdError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(CmdError { fields: Some(self.0), ..CmdError::new("invalid", "Please fix the highlighted fields.") })
        }
    }
}

// ---------- dates (Philippine calendar days) ----------

fn ph() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).expect("valid offset")
}

pub fn ph_day_bounds(day: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    let start = ph()
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).expect("midnight"))
        .single()
        .expect("fixed offset is unambiguous")
        .with_timezone(&Utc);
    (start, start + Duration::days(1))
}

fn age_on(birthdate: NaiveDate, on: NaiveDate) -> i32 {
    let mut age = on.year() - birthdate.year();
    if (on.month(), on.day()) < (birthdate.month(), birthdate.day()) {
        age -= 1;
    }
    age
}

fn date_param(label: &str, value: Option<&str>) -> Result<Option<NaiveDate>, CmdError> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) => NaiveDate::parse_from_str(v, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| bad_request(format!("{label} must be a date in YYYY-MM-DD format."))),
    }
}

// ---------- request parsing ----------

struct Query(HashMap<String, String>);

impl Query {
    fn parse(q: &str) -> Self {
        Query(url::form_urlencoded::parse(q.as_bytes()).filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.into_owned(), v.into_owned())).collect())
    }
    fn str(&self, k: &str) -> Option<&str> {
        self.0.get(k).map(String::as_str)
    }
    fn int(&self, k: &str) -> Result<Option<i64>, CmdError> {
        self.str(k).map(|v| v.trim().parse().map_err(|_| bad_request("Invalid request."))).transpose()
    }
    fn uuid(&self, k: &str) -> Result<Option<Uuid>, CmdError> {
        self.str(k).map(|v| v.trim().parse().map_err(|_| bad_request("Invalid request."))).transpose()
    }
    fn page(&self, default_size: i64) -> Result<(i64, i64), CmdError> {
        Ok((
            self.int("page")?.unwrap_or(1).clamp(1, 100_000),
            self.int("page_size")?.unwrap_or(default_size).clamp(1, MAX_PAGE_SIZE),
        ))
    }
}

fn split_path(path: &str) -> (Vec<&str>, Query) {
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    (route.trim_matches('/').split('/').collect(), Query::parse(query))
}

fn parse_id(s: &str, what: &str) -> Result<Uuid, CmdError> {
    s.parse().map_err(|_| not_found(what))
}

// ---------- search (same matching as server/src/search.rs) ----------

fn words(q: Option<&str>) -> Vec<String> {
    q.unwrap_or("")
        .chars()
        .take(MAX_QUERY_CHARS)
        .collect::<String>()
        .replace(',', " ")
        .split_whitespace()
        .map(str::to_lowercase)
        .collect()
}

/// The words appear in this order, with anything in between (like `ILIKE '%a%b%'`).
fn in_order(hay: &str, words: &[String]) -> bool {
    let mut rest = hay;
    words.iter().all(|w| match rest.find(w.as_str()) {
        Some(i) => {
            rest = &rest[i + w.len()..];
            true
        }
        None => false,
    })
}

/// Name (either "first middle last" or "last first middle") or, for a single number, LRN prefix.
fn student_matches(s: &MStudent, words: &[String]) -> bool {
    if words.is_empty() {
        return true;
    }
    if let [one] = words {
        if one.chars().all(|c| c.is_ascii_digit()) && s.lrn.starts_with(one.as_str()) {
            return true;
        }
    }
    let a = match &s.middle_name {
        Some(m) => format!("{} {} {}", s.first_name, m, s.last_name),
        None => format!("{} {}", s.first_name, s.last_name),
    };
    let b = match &s.middle_name {
        Some(m) => format!("{} {} {}", s.last_name, s.first_name, m),
        None => format!("{} {}", s.last_name, s.first_name),
    };
    in_order(&a.to_lowercase(), words) || in_order(&b.to_lowercase(), words)
}

fn student_order(a: &MStudent, b: &MStudent) -> std::cmp::Ordering {
    (a.last_name.to_lowercase(), a.first_name.to_lowercase(), a.id).cmp(&(b.last_name.to_lowercase(), b.first_name.to_lowercase(), b.id))
}

fn visit_order(a: &MVisit, b: &MVisit) -> std::cmp::Ordering {
    b.visited_at.cmp(&a.visited_at).then(a.id.cmp(&b.id))
}

// ---------- JSON shapes (as the server sends them) ----------

fn section_of<'a>(m: &'a Mirror, id: Uuid) -> MSection {
    m.sections.get(&id).cloned().unwrap_or(MSection { id, grade_level: 0, section_name: "?".into() })
}

fn student_json(m: &Mirror, s: &MStudent, today: NaiveDate) -> Value {
    let sec = section_of(m, s.section_id);
    json!({
        "id": s.id, "lrn": s.lrn, "first_name": s.first_name, "middle_name": s.middle_name,
        "last_name": s.last_name, "gender": s.gender, "birthdate": s.birthdate,
        "age": age_on(s.birthdate, today),
        "section": { "id": sec.id, "grade_level": sec.grade_level, "section_name": sec.section_name },
        "created_at": s.created_at, "updated_at": s.updated_at,
    })
}

fn staff_name(m: &Mirror, ctx: &Ctx, id: Uuid) -> String {
    match m.staff.get(&id) {
        Some(s) => s.full_name.clone(),
        None if id == ctx.user_id => ctx.user_name.to_string(),
        None => "Unknown".into(),
    }
}

fn medicine_of(m: &Mirror, id: Uuid) -> MMedicine {
    m.medicines.get(&id).cloned().unwrap_or(MMedicine { id, name: "Unknown medicine".into(), unit: "".into() })
}

/// Medicine lines with names, sorted by name.
fn lines_json(m: &Mirror, lines: &[MLine], with_id: bool) -> Value {
    let mut out: Vec<(String, Value)> = lines
        .iter()
        .map(|l| {
            let med = medicine_of(m, l.medicine_id);
            let v = if with_id {
                json!({ "medicine_id": med.id, "name": med.name, "unit": med.unit, "quantity": l.quantity })
            } else {
                json!({ "name": med.name, "unit": med.unit, "quantity": l.quantity })
            };
            (med.name, v)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Value::Array(out.into_iter().map(|(_, v)| v).collect())
}

fn visit_json(m: &Mirror, ctx: &Ctx, v: &MVisit) -> Value {
    let (student, sec) = match m.students.get(&v.student_id) {
        Some(s) => (json!({
            "id": s.id, "lrn": s.lrn, "first_name": s.first_name, "middle_name": s.middle_name, "last_name": s.last_name,
        }), section_of(m, s.section_id)),
        None => (json!({ "id": v.student_id, "lrn": "", "first_name": "Unknown", "middle_name": null, "last_name": "student" }),
                 section_of(m, Uuid::nil())),
    };
    let mut student = student;
    student["grade_level"] = json!(sec.grade_level);
    student["section_name"] = json!(sec.section_name);
    json!({
        "id": v.id, "visited_at": v.visited_at, "complaint": v.complaint, "treatment": v.treatment,
        "created_at": v.created_at, "student": student,
        "attended_by": { "id": v.user_id, "full_name": staff_name(m, ctx, v.user_id) },
        "medicines": lines_json(m, &v.medicines, true),
    })
}

fn student_label(s: &MStudent) -> String {
    format!("{}, {}", s.last_name, s.first_name)
}

fn page_json(items: Vec<Value>, total: usize, page: i64, page_size: i64) -> Value {
    json!({ "items": items, "total": total, "page": page, "page_size": page_size })
}

fn paginate<T>(items: Vec<T>, page: i64, page_size: i64) -> (Vec<T>, usize) {
    let total = items.len();
    let skip = ((page - 1) * page_size) as usize;
    (items.into_iter().skip(skip).take(page_size as usize).collect(), total)
}

// ---------- reads ----------

/// This GET can be answered offline.
pub fn can_read(path: &str) -> bool {
    let (seg, _) = split_path(path);
    matches!(
        seg.as_slice(),
        ["sections" | "medicines" | "students" | "visits" | "dashboard"] | ["students" | "visits", _] | ["reports", "basic" | "detailed"]
    )
}

/// `None` when this request can't be answered offline.
pub fn read(m: &Mirror, ctx: &Ctx, path: &str) -> Option<Result<Answer, CmdError>> {
    let (seg, q) = split_path(path);
    let plain = |r: Result<Value, CmdError>| r.map(|body| Answer { body, viewed: None });
    Some(match seg.as_slice() {
        ["sections"] => plain(Ok(sections_list(m))),
        ["medicines"] => plain(Ok(medicines_list(m))),
        ["students"] => plain(students_list(m, ctx, &q)),
        ["students", "lrn-check"] => plain(lrn_check(m, &q)),
        ["students", id] => student_detail(m, ctx, id),
        ["visits"] => plain(visits_list(m, ctx, &q)),
        ["visits", id] => parse_id(id, "Visit").and_then(|id| {
            let v = m.visits.get(&id).ok_or_else(|| not_found("Visit"))?;
            Ok(Answer { body: visit_json(m, ctx, v), viewed: Some(("clinic_visits", id)) })
        }),
        ["dashboard"] => plain(Ok(dashboard(m, ctx))),
        ["reports", "basic"] => plain(reports::basic(m, ctx, &q)),
        ["reports", "detailed"] => plain(reports::detailed(m, ctx, &q)),
        _ => return None,
    })
}

fn sections_list(m: &Mirror) -> Value {
    let mut counts: HashMap<Uuid, i64> = HashMap::new();
    for s in m.students.values() {
        *counts.entry(s.section_id).or_default() += 1;
    }
    let mut list: Vec<&MSection> = m.sections.values().collect();
    list.sort_by_key(|s| (s.grade_level, s.section_name.to_lowercase(), s.id));
    Value::Array(list.into_iter().map(|s| section_json(s, counts.get(&s.id).copied().unwrap_or(0))).collect())
}

fn section_json(s: &MSection, student_count: i64) -> Value {
    json!({ "id": s.id, "grade_level": s.grade_level, "section_name": s.section_name, "student_count": student_count })
}

fn medicine_use(m: &Mirror) -> HashMap<Uuid, i64> {
    let mut used: HashMap<Uuid, i64> = HashMap::new();
    for v in m.visits.values() {
        for l in &v.medicines {
            *used.entry(l.medicine_id).or_default() += 1;
        }
    }
    used
}

fn medicines_list(m: &Mirror) -> Value {
    let used = medicine_use(m);
    let mut list: Vec<&MMedicine> = m.medicines.values().collect();
    list.sort_by_key(|x| (x.name.to_lowercase(), x.id));
    Value::Array(list.into_iter().map(|x| medicine_json(x, used.get(&x.id).copied().unwrap_or(0))).collect())
}

fn medicine_json(x: &MMedicine, visit_count: i64) -> Value {
    json!({ "id": x.id, "name": x.name, "unit": x.unit, "visit_count": visit_count })
}

fn require_advanced(ctx: &Ctx) -> Result<(), CmdError> {
    if ctx.plan.is_some_and(|p| p.advanced_search) {
        Ok(())
    } else {
        Err(premium("Advanced search filters (date range, complaint and grade) are part of the Premium plan."))
    }
}

fn students_list(m: &Mirror, ctx: &Ctx, q: &Query) -> Result<Value, CmdError> {
    let (page, page_size) = q.page(25)?;
    let grade = q.int("grade_level")?;
    if grade.is_some() {
        require_advanced(ctx)?;
    }
    let section = q.uuid("section_id")?;
    let w = words(q.str("q"));
    let mut list: Vec<&MStudent> = m
        .students
        .values()
        .filter(|s| section.is_none_or(|id| s.section_id == id))
        .filter(|s| grade.is_none_or(|g| m.sections.get(&s.section_id).is_some_and(|sec| sec.grade_level as i64 == g)))
        .filter(|s| student_matches(s, &w))
        .collect();
    list.sort_by(|a, b| student_order(a, b));
    let (items, total) = paginate(list, page, page_size);
    let today = ctx.today();
    Ok(page_json(items.into_iter().map(|s| student_json(m, s, today)).collect(), total, page, page_size))
}

fn lrn_check(m: &Mirror, q: &Query) -> Result<Value, CmdError> {
    let mut e = Fields::default();
    let lrn = check_lrn(&mut e, q.str("lrn").unwrap_or(""));
    e.finish()?;
    let exclude = q.uuid("exclude")?;
    let owner = m.students.values().find(|s| s.lrn == lrn && Some(s.id) != exclude);
    Ok(json!({
        "available": owner.is_none(),
        "student": owner.map(|s| json!({ "id": s.id, "first_name": s.first_name, "last_name": s.last_name })),
    }))
}

fn student_detail(m: &Mirror, ctx: &Ctx, id: &str) -> Result<Answer, CmdError> {
    let id = parse_id(id, "Student")?;
    let s = m.students.get(&id).ok_or_else(|| not_found("Student"))?;
    let mut body = student_json(m, s, ctx.today());
    let mut visits: Vec<&MVisit> = m.visits.values().filter(|v| v.student_id == id).collect();
    visits.sort_by(|a, b| visit_order(a, b));
    body["visits"] = Value::Array(
        visits
            .into_iter()
            .map(|v| {
                json!({
                    "id": v.id, "visited_at": v.visited_at, "complaint": v.complaint, "treatment": v.treatment,
                    "attended_by": staff_name(m, ctx, v.user_id), "medicines": lines_json(m, &v.medicines, false),
                })
            })
            .collect(),
    );
    Ok(Answer { body, viewed: Some(("students", id)) })
}

fn visits_list(m: &Mirror, ctx: &Ctx, q: &Query) -> Result<Value, CmdError> {
    let (page, page_size) = q.page(25)?;
    let day = date_param("Date", q.str("date"))?;
    let date_from = date_param("From date", q.str("date_from"))?;
    let date_to = date_param("To date", q.str("date_to"))?;
    let complaint: Vec<String> = words(q.str("complaint")).into_iter().take(MAX_KEYWORDS).collect();
    let grade = q.int("grade_level")?;
    if grade.is_some() || date_from.is_some() || date_to.is_some() || !complaint.is_empty() {
        require_advanced(ctx)?;
    }
    if let (Some(a), Some(b)) = (date_from, date_to) {
        if a > b {
            return Err(bad_request("The 'from' date must be on or before the 'to' date."));
        }
    }
    let from = [day, date_from].into_iter().flatten().map(|d| ph_day_bounds(d).0).max();
    let to = [day, date_to].into_iter().flatten().map(|d| ph_day_bounds(d).1).min();
    let student = q.uuid("student_id")?;
    let section = q.uuid("section_id")?;
    let w = words(q.str("q"));

    let mut list: Vec<&MVisit> = m
        .visits
        .values()
        .filter(|v| from.is_none_or(|f| v.visited_at >= f) && to.is_none_or(|t| v.visited_at < t))
        .filter(|v| student.is_none_or(|s| v.student_id == s))
        .filter(|v| {
            let st = m.students.get(&v.student_id);
            let sec = st.and_then(|s| m.sections.get(&s.section_id));
            section.is_none_or(|id| st.is_some_and(|s| s.section_id == id))
                && grade.is_none_or(|g| sec.is_some_and(|s| s.grade_level as i64 == g))
                && (w.is_empty() || st.is_some_and(|s| student_matches(s, &w)))
        })
        .filter(|v| {
            let c = v.complaint.to_lowercase();
            complaint.iter().all(|k| c.contains(k.as_str()))
        })
        .collect();
    list.sort_by(|a, b| visit_order(a, b));
    let (items, total) = paginate(list, page, page_size);
    Ok(page_json(items.into_iter().map(|v| visit_json(m, ctx, v)).collect(), total, page, page_size))
}

fn dashboard(m: &Mirror, ctx: &Ctx) -> Value {
    let today = ctx.today();
    let (from, to) = ph_day_bounds(today);
    let today_count = m.visits.values().filter(|v| v.visited_at >= from && v.visited_at < to).count();
    let mut recent: Vec<&MVisit> = m.visits.values().collect();
    recent.sort_by(|a, b| visit_order(a, b));
    recent.truncate(10);
    json!({
        "today": today, "today_count": today_count,
        "recent": recent.into_iter().map(|v| visit_json(m, ctx, v)).collect::<Vec<_>>(),
    })
}

// ---------- field checks (same rules and messages as server/src/validate.rs) ----------

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn is_name_char(c: char) -> bool {
    c.is_alphabetic() || matches!(c, ' ' | '-' | '\'' | '\u{2019}' | '.')
}

fn check_name(e: &mut Fields, field: &'static str, label: &str, value: &str) -> String {
    let v = value.trim().to_string();
    let len = v.chars().count();
    if len == 0 {
        e.add(field, format!("Enter the {label}."));
    } else if len > NAME_MAX {
        e.add(field, format!("{} must be {NAME_MAX} characters or fewer.", capitalize(label)));
    } else if !v.chars().all(is_name_char) || !v.chars().any(char::is_alphabetic) {
        e.add(field, format!("{} can only have letters, spaces, hyphens (-), apostrophes (') and periods (.).", capitalize(label)));
    }
    v
}

fn check_lrn(e: &mut Fields, value: &str) -> String {
    let v = value.trim().to_string();
    if v.is_empty() {
        e.add("lrn", "Enter the LRN.");
    } else if v.len() != 12 || !v.chars().all(|c| c.is_ascii_digit()) {
        e.add("lrn", "LRN must be exactly 12 digits.");
    }
    v
}

fn check_birthdate(e: &mut Fields, value: &str, today: NaiveDate) -> Option<NaiveDate> {
    let v = value.trim();
    if v.is_empty() {
        e.add("birthdate", "Enter the birthdate.");
        return None;
    }
    let Ok(d) = NaiveDate::parse_from_str(v, "%Y-%m-%d") else {
        e.add("birthdate", "Enter a valid date.");
        return None;
    };
    if d > today {
        e.add("birthdate", "Birthdate can't be in the future.");
    } else {
        let age = age_on(d, today);
        if !(MIN_AGE..=MAX_AGE).contains(&age) {
            e.add("birthdate", format!("Age must be between {MIN_AGE} and {MAX_AGE} (this birthdate gives {age})."));
        }
    }
    Some(d)
}

fn check_text(e: &mut Fields, field: &'static str, label: &str, value: &str, min: usize, max: usize) -> String {
    let v = value.trim().to_string();
    let len = v.chars().count();
    if len == 0 {
        e.add(field, format!("Enter the {label}."));
    } else if len < min {
        e.add(field, format!("{} must be at least {min} characters.", capitalize(label)));
    } else if len > max {
        e.add(field, format!("{} must be {max} characters or fewer.", capitalize(label)));
    } else if v.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        e.add(field, format!("{} has characters that aren't allowed.", capitalize(label)));
    }
    v
}

/// Medicine name / unit (server/src/medicines.rs).
fn check_short(e: &mut Fields, field: &'static str, label: &str, value: &str, max: usize) -> String {
    let v = value.trim().to_string();
    let len = v.chars().count();
    if len == 0 {
        e.add(field, format!("Enter the {label}."));
    } else if len > max {
        e.add(field, format!("Must be {max} characters or fewer."));
    } else if v.chars().any(char::is_control) {
        e.add(field, "Has characters that aren't allowed.");
    }
    v
}

fn check_section_name(e: &mut Fields, value: &str) -> String {
    let v = value.trim().to_string();
    let len = v.chars().count();
    if len == 0 {
        e.add("section_name", "Enter the section name.");
    } else if len > 50 {
        e.add("section_name", "Section name must be 50 characters or fewer.");
    } else if v.chars().any(char::is_control) {
        e.add("section_name", "Section name has characters that aren't allowed.");
    }
    v
}

fn str_field<'a>(body: &'a Value, k: &str) -> &'a str {
    body.get(k).and_then(Value::as_str).unwrap_or("")
}

fn uuid_field(body: &Value, k: &str) -> Option<Uuid> {
    body.get(k).and_then(Value::as_str).and_then(|s| s.parse().ok())
}

// ---------- writes ----------

/// This change can be made offline (not user accounts or payments, for example).
pub fn can_write(method: &str, path: &str) -> bool {
    let (seg, _) = split_path(path);
    matches!(
        (method, seg.as_slice()),
        ("POST", ["students" | "visits" | "sections" | "medicines"]) | ("PUT" | "DELETE", ["students" | "visits" | "sections" | "medicines", _])
    )
}

/// `None` when this change can't be made offline (see [`can_write`]).
pub fn write(m: &Mirror, ctx: &Ctx, method: &str, path: &str, body: Option<&Value>) -> Option<Result<Written, CmdError>> {
    if !can_write(method, path) {
        return None;
    }
    let (seg, _) = split_path(path);
    let empty = Value::Object(Map::new());
    let body = body.unwrap_or(&empty);
    Some(allowed(ctx).and_then(|()| match (method, seg.as_slice()) {
        ("POST", ["students"]) => students::create(m, ctx, body),
        ("PUT", ["students", id]) => parse_id(id, "Student").and_then(|id| students::update(m, ctx, id, body)),
        ("DELETE", ["students", id]) => parse_id(id, "Student").and_then(|id| students::delete(m, ctx, id)),
        ("POST", ["visits"]) => visits::create(m, ctx, body),
        ("PUT", ["visits", id]) => parse_id(id, "Visit").and_then(|id| visits::update(m, ctx, id, body)),
        ("DELETE", ["visits", id]) => parse_id(id, "Visit").and_then(|id| visits::delete(m, ctx, id)),
        ("POST", ["sections"]) => sections::save(m, ctx, None, body),
        ("PUT", ["sections", id]) => parse_id(id, "Section").and_then(|id| sections::save(m, ctx, Some(id), body)),
        ("DELETE", ["sections", id]) => parse_id(id, "Section").and_then(|id| sections::delete(m, ctx, id)),
        ("POST", ["medicines"]) => medicines::save(m, ctx, None, body),
        ("PUT", ["medicines", id]) => parse_id(id, "Medicine").and_then(|id| medicines::save(m, ctx, Some(id), body)),
        ("DELETE", ["medicines", id]) => parse_id(id, "Medicine").and_then(|id| medicines::delete(m, ctx, id)),
        _ => unreachable!("checked above"),
    }))
}

/// Changes are refused while the plan is read-only (expired, or offline past the grace period).
fn allowed(ctx: &Ctx) -> Result<(), CmdError> {
    if ctx.replay {
        return Ok(());
    }
    match ctx.plan {
        Some(p) if p.grace_expired => Err(coded(
            "forbidden",
            "read_only",
            "This PC has been offline for more than 7 days, so records are view-only until it reconnects to the server.",
            None,
        )),
        Some(p) if !p.read_only => Ok(()),
        _ => Err(coded(
            "forbidden",
            "read_only",
            "Your school's subscription has expired. Records can be viewed but not changed until it is renewed.",
            None,
        )),
    }
}

fn admin_only(ctx: &Ctx) -> Result<(), CmdError> {
    if ctx.replay || ctx.is_admin {
        Ok(())
    } else {
        Err(forbidden("Only an admin can do this."))
    }
}

/// The client-made id of a new record (made here if the app didn't send one).
fn new_id(body: &Value) -> Uuid {
    uuid_field(body, "id").unwrap_or_else(Uuid::new_v4)
}

fn with_field(body: &Value, k: &str, v: Value) -> Value {
    let mut b = body.clone();
    if let Some(o) = b.as_object_mut() {
        o.insert(k.into(), v);
    }
    b
}

mod students {
    use super::*;

    struct Clean {
        lrn: String,
        first_name: String,
        middle_name: Option<String>,
        last_name: String,
        gender: String,
        birthdate: NaiveDate,
        section_id: Uuid,
    }

    fn validate(m: &Mirror, ctx: &Ctx, self_id: Option<Uuid>, b: &Value) -> Result<Clean, CmdError> {
        let mut e = Fields::default();
        let lrn = check_lrn(&mut e, str_field(b, "lrn"));
        let first_name = check_name(&mut e, "first_name", "first name", str_field(b, "first_name"));
        let middle_name = match b.get("middle_name").and_then(Value::as_str).map(str::trim) {
            None | Some("") => None,
            Some(v) => Some(check_name(&mut e, "middle_name", "middle name", v)),
        };
        let last_name = check_name(&mut e, "last_name", "last name", str_field(b, "last_name"));
        let gender = str_field(b, "gender").trim().to_lowercase();
        if gender != "male" && gender != "female" {
            e.add("gender", "Choose a gender.");
        }
        let birthdate = check_birthdate(&mut e, str_field(b, "birthdate"), ctx.today());
        let section_id = uuid_field(b, "section_id");
        match section_id {
            None => e.add("section_id", "Choose a section."),
            Some(sid) if !m.sections.contains_key(&sid) => e.add("section_id", "Choose a section from the list."),
            Some(_) => {}
        }
        if !e.has("lrn") {
            if let Some(t) = m.students.values().find(|s| s.lrn == lrn && Some(s.id) != self_id) {
                e.add("lrn", format!("This LRN is already registered to {}, {}.", t.last_name, t.first_name));
            }
        }
        e.finish()?;
        Ok(Clean {
            lrn,
            first_name,
            middle_name,
            last_name,
            gender,
            birthdate: birthdate.expect("checked"),
            section_id: section_id.expect("checked"),
        })
    }

    /// Warn-don't-block, like the server's 409 `possible_duplicate`.
    fn check_duplicate(m: &Mirror, self_id: Option<Uuid>, c: &Clean) -> Result<(), CmdError> {
        let mut matches: Vec<&MStudent> = m
            .students
            .values()
            .filter(|s| {
                Some(s.id) != self_id
                    && s.first_name.to_lowercase() == c.first_name.to_lowercase()
                    && s.last_name.to_lowercase() == c.last_name.to_lowercase()
                    && s.birthdate == c.birthdate
            })
            .collect();
        if matches.is_empty() {
            return Ok(());
        }
        matches.sort_by(|a, b| student_order(a, b));
        matches.truncate(5);
        let list: Vec<Value> = matches
            .into_iter()
            .map(|s| {
                let sec = section_of(m, s.section_id);
                json!({ "id": s.id, "lrn": s.lrn, "first_name": s.first_name, "last_name": s.last_name,
                        "grade_level": sec.grade_level, "section_name": sec.section_name })
            })
            .collect();
        Err(coded(
            "conflict",
            "possible_duplicate",
            "A student with the same name and birthdate is already registered.",
            Some(json!({ "matches": list })),
        ))
    }

    fn allow_duplicate(b: &Value) -> bool {
        b.get("allow_duplicate").and_then(Value::as_bool).unwrap_or(false)
    }

    pub fn create(m: &Mirror, ctx: &Ctx, b: &Value) -> Result<Written, CmdError> {
        let id = new_id(b);
        let body = with_field(b, "id", json!(id));
        if let Some(s) = m.students.get(&id) {
            // Already saved (a retried request).
            return Ok(Written {
                response: Some(student_json(m, s, ctx.today())),
                changes: vec![],
                path: "students".into(),
                body: Some(body),
                action: "New student",
                label: student_label(s),
                summary: String::new(),
            });
        }
        let c = validate(m, ctx, None, b)?;
        if !ctx.replay {
            if let Some(max) = ctx.plan.and_then(|p| p.max_students) {
                if m.students.len() as i64 >= max {
                    return Err(forbidden(format!(
                        "Your Basic plan allows up to {max} student records. Upgrade to Premium for unlimited records."
                    )));
                }
            }
            if !allow_duplicate(b) {
                check_duplicate(m, None, &c)?;
            }
        }
        let s = MStudent {
            id,
            lrn: c.lrn,
            first_name: c.first_name,
            middle_name: c.middle_name,
            last_name: c.last_name,
            gender: c.gender,
            birthdate: c.birthdate,
            section_id: c.section_id,
            created_at: ctx.now,
            updated_at: ctx.now,
        };
        let sec = section_of(m, s.section_id);
        Ok(Written {
            response: Some(student_json(m, &s, ctx.today())),
            path: "students".into(),
            body: Some(body),
            action: "New student",
            label: student_label(&s),
            summary: format!("LRN {} · Grade {} – {}", s.lrn, sec.grade_level, sec.section_name),
            changes: vec![Change::Student(s)],
        })
    }

    pub fn update(m: &Mirror, ctx: &Ctx, id: Uuid, b: &Value) -> Result<Written, CmdError> {
        let old = m.students.get(&id).ok_or_else(|| not_found("Student"))?;
        let c = validate(m, ctx, Some(id), b)?;
        let identity_changed = old.first_name.to_lowercase() != c.first_name.to_lowercase()
            || old.last_name.to_lowercase() != c.last_name.to_lowercase()
            || old.birthdate != c.birthdate;
        if identity_changed && !allow_duplicate(b) && !ctx.replay {
            check_duplicate(m, Some(id), &c)?;
        }
        let s = MStudent {
            lrn: c.lrn,
            first_name: c.first_name,
            middle_name: c.middle_name,
            last_name: c.last_name,
            gender: c.gender,
            birthdate: c.birthdate,
            section_id: c.section_id,
            updated_at: ctx.now,
            ..old.clone()
        };
        Ok(Written {
            response: Some(student_json(m, &s, ctx.today())),
            path: format!("students/{id}"),
            body: Some(b.clone()),
            action: "Student record edited",
            label: student_label(&s),
            summary: String::new(),
            changes: vec![Change::Student(s)],
        })
    }

    pub fn delete(m: &Mirror, ctx: &Ctx, id: Uuid) -> Result<Written, CmdError> {
        admin_only(ctx)?;
        let s = m.students.get(&id).ok_or_else(|| not_found("Student"))?;
        let visits = m.visits.values().filter(|v| v.student_id == id).count();
        if visits > 0 {
            return Err(coded(
                "conflict",
                "student_has_visits",
                format!(
                    "This student has {visits} clinic visit{} on record, so the student can't be deleted.",
                    if visits == 1 { "" } else { "s" }
                ),
                None,
            ));
        }
        Ok(Written {
            response: None,
            changes: vec![Change::Delete(Table::Students, id)],
            path: format!("students/{id}"),
            body: None,
            action: "Student deleted",
            label: student_label(s),
            summary: String::new(),
        })
    }
}

mod visits {
    use super::*;

    /// Complaint, treatment and medicine lines (server/src/visits.rs `validate_body`).
    fn validate_body(m: &Mirror, b: &Value, e: &mut Fields) -> (String, String, Vec<MLine>) {
        let complaint = check_text(e, "complaint", "complaint", str_field(b, "complaint"), 3, 500);
        let treatment = check_text(e, "treatment", "treatment", str_field(b, "treatment"), 1, 500);
        let lines = b.get("medicines").and_then(Value::as_array).cloned().unwrap_or_default();
        if lines.len() > MAX_MEDICINE_LINES {
            e.add("medicines", format!("A visit can list up to {MAX_MEDICINE_LINES} medicines."));
            return (complaint, treatment, vec![]);
        }
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let qty = match line.get("quantity").and_then(Value::as_i64) {
                Some(q) if (1..=100).contains(&q) => q as i32,
                Some(_) => {
                    e.add(format!("medicines.{i}.quantity"), "Quantity must be from 1 to 100.");
                    0
                }
                None => {
                    e.add(format!("medicines.{i}.quantity"), "Quantity must be a whole number.");
                    0
                }
            };
            match uuid_field(line, "medicine_id") {
                None => e.add(format!("medicines.{i}.medicine_id"), "Choose a medicine."),
                Some(id) if !m.medicines.contains_key(&id) => {
                    e.add(format!("medicines.{i}.medicine_id"), "Choose a medicine from the list.")
                }
                Some(id) if !seen.insert(id) => e.add(format!("medicines.{i}.medicine_id"), "This medicine is already listed."),
                Some(id) => out.push(MLine { medicine_id: id, quantity: qty }),
            }
        }
        (complaint, treatment, out)
    }

    fn label(m: &Mirror, v: &MVisit) -> String {
        m.students.get(&v.student_id).map(student_label).unwrap_or_else(|| "Unknown student".into())
    }

    pub fn create(m: &Mirror, ctx: &Ctx, b: &Value) -> Result<Written, CmdError> {
        let id = new_id(b);
        if let Some(v) = m.visits.get(&id) {
            return Ok(Written {
                response: Some(visit_json(m, ctx, v)),
                changes: vec![],
                path: "visits".into(),
                body: Some(with_field(b, "id", json!(id))),
                action: "New visit",
                label: label(m, v),
                summary: v.complaint.clone(),
            });
        }
        let mut e = Fields::default();
        let student_id = uuid_field(b, "student_id");
        match student_id {
            None => e.add("student_id", "Choose a student."),
            Some(sid) if !m.students.contains_key(&sid) => e.add("student_id", "Choose a student from the list."),
            Some(_) => {}
        }
        // A visit saved offline keeps the time it was recorded, not the time it uploads.
        let given: Option<DateTime<Utc>> = b.get("visited_at").and_then(Value::as_str).and_then(|t| t.parse().ok());
        let visited_at = match given {
            None => ctx.now,
            Some(t) if ctx.replay => t,
            Some(t) if t > ctx.now + Duration::minutes(CLOCK_SKEW_MINUTES) => {
                e.add("visited_at", "Visit time can't be in the future.");
                ctx.now
            }
            Some(t) if t < ctx.now - Duration::days(MAX_BACKDATE_DAYS) => {
                e.add("visited_at", format!("Visit time can't be more than {MAX_BACKDATE_DAYS} days ago."));
                ctx.now
            }
            Some(t) => t.min(ctx.now),
        };
        let (complaint, treatment, lines) = validate_body(m, b, &mut e);
        e.finish()?;
        let v = MVisit {
            id,
            student_id: student_id.expect("checked"),
            user_id: ctx.user_id,
            visited_at,
            complaint,
            treatment,
            created_at: ctx.now,
            updated_at: ctx.now,
            medicines: lines,
        };
        let mut body = with_field(b, "id", json!(id));
        body = with_field(&body, "visited_at", json!(visited_at));
        Ok(Written {
            response: Some(visit_json(m, ctx, &v)),
            path: "visits".into(),
            body: Some(body),
            action: "New visit",
            label: label(m, &v),
            summary: v.complaint.clone(),
            changes: vec![Change::Visit(v)],
        })
    }

    pub fn update(m: &Mirror, ctx: &Ctx, id: Uuid, b: &Value) -> Result<Written, CmdError> {
        let old = m.visits.get(&id).ok_or_else(|| not_found("Visit"))?;
        if !ctx.replay && !ctx.is_admin && old.user_id != ctx.user_id {
            return Err(forbidden("Only the staff member who recorded this visit, or an admin, can edit it."));
        }
        let mut e = Fields::default();
        if uuid_field(b, "student_id").is_some_and(|s| s != old.student_id) {
            e.add("student_id", "The student on a visit can't be changed. Delete it and record a new visit instead.");
        }
        let (complaint, treatment, lines) = validate_body(m, b, &mut e);
        e.finish()?;
        let v = MVisit { complaint, treatment, medicines: lines, updated_at: ctx.now, ..old.clone() };
        Ok(Written {
            response: Some(visit_json(m, ctx, &v)),
            path: format!("visits/{id}"),
            body: Some(b.clone()),
            action: "Visit edited",
            label: label(m, &v),
            summary: v.complaint.clone(),
            changes: vec![Change::Visit(v)],
        })
    }

    pub fn delete(m: &Mirror, ctx: &Ctx, id: Uuid) -> Result<Written, CmdError> {
        admin_only(ctx)?;
        let v = m.visits.get(&id).ok_or_else(|| not_found("Visit"))?;
        Ok(Written {
            response: None,
            changes: vec![Change::Delete(Table::Visits, id)],
            path: format!("visits/{id}"),
            body: None,
            action: "Visit deleted",
            label: label(m, v),
            summary: v.complaint.clone(),
        })
    }
}

mod sections {
    use super::*;

    fn name(s: &MSection) -> String {
        format!("Grade {} – {}", s.grade_level, s.section_name)
    }

    pub fn save(m: &Mirror, ctx: &Ctx, existing: Option<Uuid>, b: &Value) -> Result<Written, CmdError> {
        admin_only(ctx)?;
        let id = match existing {
            Some(id) => {
                m.sections.get(&id).ok_or_else(|| not_found("Section"))?;
                id
            }
            None => new_id(b),
        };
        let mut e = Fields::default();
        let grade = match b.get("grade_level").and_then(Value::as_i64) {
            Some(g) if (1..=12).contains(&g) => g as i32,
            _ => {
                e.add("grade_level", "Grade level must be from 1 to 12.");
                0
            }
        };
        let section_name = check_section_name(&mut e, str_field(b, "section_name"));
        if existing.is_none() && m.sections.contains_key(&id) {
            let s = &m.sections[&id];
            return Ok(Written {
                response: Some(section_json(s, count(m, id))),
                changes: vec![],
                path: "sections".into(),
                body: Some(with_field(b, "id", json!(id))),
                action: "New section",
                label: name(s),
                summary: String::new(),
            });
        }
        e.finish()?;
        if m.sections.values().any(|s| s.id != id && s.grade_level == grade && s.section_name == section_name) {
            let mut e = Fields::default();
            e.add("section_name", "This grade already has a section with that name.");
            e.finish()?;
        }
        let s = MSection { id, grade_level: grade, section_name };
        Ok(Written {
            response: Some(section_json(&s, count(m, id))),
            path: if existing.is_some() { format!("sections/{id}") } else { "sections".into() },
            body: Some(if existing.is_some() { b.clone() } else { with_field(b, "id", json!(id)) }),
            action: if existing.is_some() { "Section renamed" } else { "New section" },
            label: name(&s),
            summary: String::new(),
            changes: vec![Change::Section(s)],
        })
    }

    fn count(m: &Mirror, id: Uuid) -> i64 {
        m.students.values().filter(|s| s.section_id == id).count() as i64
    }

    pub fn delete(m: &Mirror, ctx: &Ctx, id: Uuid) -> Result<Written, CmdError> {
        admin_only(ctx)?;
        let s = m.sections.get(&id).ok_or_else(|| not_found("Section"))?;
        let n = count(m, id);
        if n > 0 {
            return Err(coded(
                "conflict",
                "section_in_use",
                format!(
                    "Grade {} – {} still has {n} student{}. Move them to another section first.",
                    s.grade_level,
                    s.section_name,
                    if n == 1 { "" } else { "s" }
                ),
                None,
            ));
        }
        Ok(Written {
            response: None,
            changes: vec![Change::Delete(Table::Sections, id)],
            path: format!("sections/{id}"),
            body: None,
            action: "Section deleted",
            label: name(s),
            summary: String::new(),
        })
    }
}

mod medicines {
    use super::*;

    pub fn save(m: &Mirror, ctx: &Ctx, existing: Option<Uuid>, b: &Value) -> Result<Written, CmdError> {
        admin_only(ctx)?;
        let id = match existing {
            Some(id) => {
                m.medicines.get(&id).ok_or_else(|| not_found("Medicine"))?;
                id
            }
            None => new_id(b),
        };
        if existing.is_none() {
            if let Some(x) = m.medicines.get(&id) {
                return Ok(Written {
                    response: Some(medicine_json(x, medicine_use(m).get(&id).copied().unwrap_or(0))),
                    changes: vec![],
                    path: "medicines".into(),
                    body: Some(with_field(b, "id", json!(id))),
                    action: "New medicine",
                    label: x.name.clone(),
                    summary: String::new(),
                });
            }
        }
        let mut e = Fields::default();
        let name = check_short(&mut e, "name", "medicine name", str_field(b, "name"), 100);
        let unit = check_short(&mut e, "unit", "unit (e.g. tablet, ml, sachet)", str_field(b, "unit"), 30);
        if !e.has("name") && m.medicines.values().any(|x| x.id != id && x.name.to_lowercase() == name.to_lowercase()) {
            e.add("name", "A medicine with this name already exists.");
        }
        e.finish()?;
        let x = MMedicine { id, name, unit };
        Ok(Written {
            response: Some(medicine_json(&x, medicine_use(m).get(&id).copied().unwrap_or(0))),
            path: if existing.is_some() { format!("medicines/{id}") } else { "medicines".into() },
            body: Some(if existing.is_some() { b.clone() } else { with_field(b, "id", json!(id)) }),
            action: if existing.is_some() { "Medicine edited" } else { "New medicine" },
            label: x.name.clone(),
            summary: x.unit.clone(),
            changes: vec![Change::Medicine(x)],
        })
    }

    pub fn delete(m: &Mirror, ctx: &Ctx, id: Uuid) -> Result<Written, CmdError> {
        admin_only(ctx)?;
        let x = m.medicines.get(&id).ok_or_else(|| not_found("Medicine"))?;
        if medicine_use(m).get(&id).copied().unwrap_or(0) > 0 {
            return Err(coded(
                "conflict",
                "medicine_in_use",
                format!("{} was given in past visits, so it can't be deleted. You can rename it instead.", x.name),
                None,
            ));
        }
        Ok(Written {
            response: None,
            changes: vec![Change::Delete(Table::Medicines, id)],
            path: format!("medicines/{id}"),
            body: None,
            action: "Medicine deleted",
            label: x.name.clone(),
            summary: String::new(),
        })
    }
}

// ---------- reports (server/src/reports.rs) ----------

mod reports {
    use super::*;

    const TOP_COMPLAINTS: usize = 10;
    const MIN_COMPLAINT_VISITS: i64 = 2;
    const TOP_VISITORS: usize = 10;

    struct Period {
        year: i32,
        month: Option<u32>,
        start: NaiveDate,
        end: NaiveDate,
    }

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
    }

    impl Period {
        fn from_query(q: &Query, today: NaiveDate) -> Result<Self, CmdError> {
            let this_year = today.year();
            let year = q.int("year")?.map(|y| y as i32).unwrap_or(this_year);
            if !(2000..=this_year + 1).contains(&year) {
                return Err(bad_request(format!("Year must be from 2000 to {}.", this_year + 1)));
            }
            let month = q.int("month")?;
            let (start, end) = match month {
                None => (ymd(year, 1, 1), ymd(year, 12, 31)),
                Some(m @ 1..=12) => {
                    let m = m as u32;
                    let next = if m == 12 { ymd(year + 1, 1, 1) } else { ymd(year, m + 1, 1) };
                    (ymd(year, m, 1), next.pred_opt().expect("valid date"))
                }
                Some(_) => return Err(bad_request("Month must be from 1 to 12.")),
            };
            Ok(Self { year, month: month.map(|m| m as u32), start, end })
        }

        fn json(&self) -> Value {
            json!({ "year": self.year, "month": self.month, "start": self.start, "end": self.end })
        }

        fn contains(&self, at: DateTime<Utc>) -> bool {
            at >= ph_day_bounds(self.start).0 && at < ph_day_bounds(self.end).1
        }
    }

    pub fn basic(m: &Mirror, ctx: &Ctx, q: &Query) -> Result<Value, CmdError> {
        let p = Period::from_query(q, ctx.today())?;
        let in_period: Vec<&MVisit> = m.visits.values().filter(|v| p.contains(v.visited_at)).collect();
        let students: HashSet<Uuid> = in_period.iter().map(|v| v.student_id).collect();
        let mut by_month: BTreeMap<u32, i64> = BTreeMap::new();
        for v in m.visits.values() {
            let local = v.visited_at.with_timezone(&ph());
            if local.year() == p.year {
                *by_month.entry(local.month()).or_default() += 1;
            }
        }
        let per_month: Vec<Value> =
            (1..=12u32).map(|mo| json!({ "month": mo, "visits": by_month.get(&mo).copied().unwrap_or(0) })).collect();
        let top = top_terms(in_period.iter().map(|v| v.complaint.as_str()), TOP_COMPLAINTS, MIN_COMPLAINT_VISITS);
        Ok(json!({
            "period": p.json(), "total_visits": in_period.len(), "students_seen": students.len(),
            "visits_per_month": per_month,
            "top_complaints": top.into_iter().map(|(term, visits)| json!({ "term": term, "visits": visits })).collect::<Vec<_>>(),
        }))
    }

    pub fn detailed(m: &Mirror, ctx: &Ctx, q: &Query) -> Result<Value, CmdError> {
        if !ctx.plan.is_some_and(|p| p.detailed_reports) {
            return Err(premium("Detailed reports are part of the Premium plan."));
        }
        let p = Period::from_query(q, ctx.today())?;
        let in_period: Vec<&MVisit> = m.visits.values().filter(|v| p.contains(v.visited_at)).collect();

        // Visits by grade and section.
        let mut by_section: BTreeMap<(i32, String, String), (i64, HashSet<Uuid>)> = BTreeMap::new();
        for v in &in_period {
            let Some(s) = m.students.get(&v.student_id) else { continue };
            let sec = section_of(m, s.section_id);
            let e = by_section.entry((sec.grade_level, sec.section_name.to_lowercase(), sec.section_name)).or_default();
            e.0 += 1;
            e.1.insert(s.id);
        }
        let by_section: Vec<Value> = by_section
            .into_iter()
            .map(|((g, _, name), (visits, students))| {
                json!({ "grade_level": g, "section_name": name, "visits": visits, "students": students.len() })
            })
            .collect();

        // Medicine usage.
        let mut usage: HashMap<Uuid, (i64, i64)> = HashMap::new();
        for v in &in_period {
            for l in &v.medicines {
                let e = usage.entry(l.medicine_id).or_default();
                e.0 += l.quantity as i64;
                e.1 += 1;
            }
        }
        let mut meds: Vec<(MMedicine, i64, i64)> = usage.into_iter().map(|(id, (q, n))| (medicine_of(m, id), q, n)).collect();
        meds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase())));
        let medicines: Vec<Value> = meds
            .into_iter()
            .map(|(x, q, n)| json!({ "name": x.name, "unit": x.unit, "total_quantity": q, "visits": n }))
            .collect();

        // Frequent visitors.
        let mut per_student: HashMap<Uuid, (i64, DateTime<Utc>)> = HashMap::new();
        for v in &in_period {
            let e = per_student.entry(v.student_id).or_insert((0, v.visited_at));
            e.0 += 1;
            e.1 = e.1.max(v.visited_at);
        }
        let mut top: Vec<(Uuid, i64, DateTime<Utc>)> = per_student.into_iter().map(|(id, (n, last))| (id, n, last)).collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
        let frequent: Vec<Value> = top
            .into_iter()
            .filter_map(|(id, n, last)| {
                let s = m.students.get(&id)?;
                let sec = section_of(m, s.section_id);
                Some(json!({
                    "student_id": s.id, "lrn": s.lrn, "first_name": s.first_name, "middle_name": s.middle_name,
                    "last_name": s.last_name, "grade_level": sec.grade_level, "section_name": sec.section_name,
                    "visits": n, "last_visit": last,
                }))
            })
            .take(TOP_VISITORS)
            .collect();

        Ok(json!({ "period": p.json(), "by_section": by_section, "medicines": medicines, "frequent_visitors": frequent }))
    }

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

    fn tokens(text: &str) -> Vec<Option<String>> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphabetic())
            .filter(|w| !w.is_empty())
            .map(|w| (w.chars().count() >= 3 && !STOPWORDS.contains(&w)).then(|| w.to_string()))
            .collect()
    }

    /// Most common complaint terms; same rules as the server's `top_terms`.
    pub fn top_terms<'a>(complaints: impl IntoIterator<Item = &'a str>, limit: usize, min_visits: i64) -> Vec<(String, i64)> {
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
        let mut out: Vec<(String, i64)> = kept_pairs.clone();
        for (w, n) in words {
            let covered = kept_pairs.iter().any(|(p, pn)| *pn == n && p.split(' ').any(|part| part == w));
            if !covered {
                out.push((w, n));
            }
        }
        out.retain(|t| t.1 >= min_visits);
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out.truncate(limit);
        out
    }

    #[cfg(test)]
    #[test]
    fn top_terms_match_the_server() {
        assert_eq!(top_terms(["Stomach ache", "stomach ache after recess"], 10, 1), vec![("stomach ache".to_string(), 2)]);
        assert_eq!(top_terms(["Headache", "Headache during basketball", "Fever"], 10, 2), vec![("headache".to_string(), 2)]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct World {
        m: Mirror,
        me: Uuid,
        plan: PlanFlags,
        section: Uuid,
        medicine: Uuid,
    }

    fn world() -> World {
        let mut m = Mirror::default();
        let section = MSection { id: Uuid::new_v4(), grade_level: 7, section_name: "Rosal".into() };
        let medicine = MMedicine { id: Uuid::new_v4(), name: "Paracetamol".into(), unit: "tablet".into() };
        let me = Uuid::new_v4();
        m.apply(&Change::Staff(crate::mirror::MStaff { id: me, full_name: "Nurse Joy".into() }));
        let (sid, mid) = (section.id, medicine.id);
        m.apply(&Change::Section(section));
        m.apply(&Change::Medicine(medicine));
        let plan = PlanFlags { advanced_search: false, detailed_reports: false, read_only: false, grace_expired: false, max_students: Some(500) };
        World { m, me, plan, section: sid, medicine: mid }
    }

    fn ctx<'a>(w: &'a World, admin: bool) -> Ctx<'a> {
        Ctx { user_id: w.me, user_name: "Nurse Joy", is_admin: admin, plan: Some(&w.plan), now: Utc::now(), replay: false }
    }

    fn student_body(w: &World, first: &str, last: &str, lrn: &str) -> Value {
        json!({ "lrn": lrn, "first_name": first, "middle_name": "", "last_name": last, "gender": "male",
                "birthdate": "2012-05-01", "section_id": w.section })
    }

    /// Makes a change and applies it, like the app does.
    fn save(w: &mut World, admin: bool, method: &str, path: &str, body: Option<&Value>) -> Written {
        let r = write(&w.m, &ctx(w, admin), method, path, body).unwrap().unwrap();
        w.m.apply_all(&r.changes);
        r
    }

    #[test]
    fn add_a_student_and_a_visit_offline_then_read_them_back() {
        let mut w = world();
        let b = student_body(&w, "Juan", "Dela Cruz", "123456789012");
        let s = save(&mut w, false, "POST", "students", Some(&b));
        let student_id: Uuid = s.response.as_ref().unwrap()["id"].as_str().unwrap().parse().unwrap();
        assert_eq!(s.body.as_ref().unwrap()["id"], json!(student_id), "upload carries the client id");
        assert_eq!(s.response.unwrap()["section"]["section_name"], "Rosal");

        let visit = json!({ "student_id": student_id, "complaint": "Headache since morning", "treatment": "Rest",
                            "medicines": [{ "medicine_id": w.medicine, "quantity": 1 }] });
        let v = save(&mut w, false, "POST", "visits", Some(&visit));
        assert!(v.body.as_ref().unwrap()["visited_at"].is_string(), "upload keeps the time it was recorded");
        let visit_id = v.response.unwrap()["id"].as_str().unwrap().to_string();

        let c = ctx(&w, false);
        let list = read(&w.m, &c, "students?q=dela%20cruz%2C%20juan&page=1").unwrap().unwrap().body;
        assert_eq!(list["total"], 1);
        let detail = read(&w.m, &c, &format!("students/{student_id}")).unwrap().unwrap();
        assert_eq!(detail.viewed, Some(("students", student_id)));
        assert_eq!(detail.body["visits"][0]["attended_by"], "Nurse Joy");
        assert_eq!(detail.body["visits"][0]["medicines"][0]["name"], "Paracetamol");
        let dash = read(&w.m, &c, "dashboard").unwrap().unwrap().body;
        assert_eq!((dash["today_count"].as_i64(), dash["recent"][0]["id"].as_str()), (Some(1), Some(visit_id.as_str())));
        let meds = read(&w.m, &c, "medicines").unwrap().unwrap().body;
        assert_eq!(meds[0]["visit_count"], 1);
        assert_eq!(read(&w.m, &c, "visits?q=juan").unwrap().unwrap().body["total"], 1);
        assert_eq!(read(&w.m, &c, "visits?q=maria").unwrap().unwrap().body["total"], 0);
        assert_eq!(read(&w.m, &c, "reports/basic").unwrap().unwrap().body["total_visits"], 1);
        assert!(read(&w.m, &c, "users").is_none(), "admin pages aren't answered offline");
    }

    #[test]
    fn same_checks_as_the_server() {
        let mut w = world();
        let b = student_body(&w, "Juan", "Dela Cruz", "123456789012");
        save(&mut w, false, "POST", "students", Some(&b));

        // LRN taken, bad name.
        let err = write(&w.m, &ctx(&w, false), "POST", "students", Some(&student_body(&w, "Juan2", "Santos", "123456789012"))).unwrap().unwrap_err();
        let f = err.fields.unwrap();
        assert_eq!(f["lrn"], "This LRN is already registered to Dela Cruz, Juan.");
        assert!(f.contains_key("first_name"));

        // Same name and birthdate: a warning that "save anyway" gets past.
        let dup = student_body(&w, "juan", "dela cruz", "000000000001");
        let err = write(&w.m, &ctx(&w, false), "POST", "students", Some(&dup)).unwrap().unwrap_err();
        assert_eq!((err.kind, err.code.as_deref()), ("conflict", Some("possible_duplicate")));
        let ok = with_field(&dup, "allow_duplicate", json!(true));
        assert!(write(&w.m, &ctx(&w, false), "POST", "students", Some(&ok)).unwrap().is_ok());

        // Visit checks.
        let err = write(&w.m, &ctx(&w, false), "POST", "visits", Some(&json!({ "complaint": "ab", "medicines": [{ "medicine_id": null, "quantity": "x" }] })))
            .unwrap()
            .unwrap_err();
        let f = err.fields.unwrap();
        for k in ["student_id", "complaint", "treatment", "medicines.0.medicine_id", "medicines.0.quantity"] {
            assert!(f.contains_key(k), "missing {k}");
        }

        // Staff can't delete; admins can't delete a student with visits.
        let sid = *w.m.students.keys().next().unwrap();
        assert_eq!(write(&w.m, &ctx(&w, false), "DELETE", &format!("students/{sid}"), None).unwrap().unwrap_err().kind, "forbidden");
        // Premium filters and read-only plans.
        assert_eq!(read(&w.m, &ctx(&w, false), "visits?date_from=2026-01-01").unwrap().unwrap_err().code.as_deref(), Some("premium_required"));
        w.plan.read_only = true;
        let err = write(&w.m, &ctx(&w, true), "POST", "medicines", Some(&json!({ "name": "ORS", "unit": "sachet" }))).unwrap().unwrap_err();
        assert_eq!(err.code.as_deref(), Some("read_only"));
        assert!(write(&w.m, &ctx(&w, true), "POST", "users", Some(&json!({}))).is_none(), "accounts need the server");
    }

    #[test]
    fn staff_edit_only_their_own_visits_and_replays_skip_permission_checks() {
        let mut w = world();
        let b = student_body(&w, "Ana", "Reyes", "123456789013");
        save(&mut w, false, "POST", "students", Some(&b));
        let sid = *w.m.students.keys().next().unwrap();
        let someone_else = Uuid::new_v4();
        let vid = Uuid::new_v4();
        w.m.apply(&Change::Visit(MVisit {
            id: vid,
            student_id: sid,
            user_id: someone_else,
            visited_at: Utc::now(),
            complaint: "Fever".into(),
            treatment: "Rest".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            medicines: vec![],
        }));
        let edit = json!({ "student_id": sid, "complaint": "Fever and cough", "treatment": "Rest", "medicines": [] });
        assert_eq!(write(&w.m, &ctx(&w, false), "PUT", &format!("visits/{vid}"), Some(&edit)).unwrap().unwrap_err().kind, "forbidden");
        assert!(write(&w.m, &ctx(&w, true), "PUT", &format!("visits/{vid}"), Some(&edit)).unwrap().is_ok(), "admins can");
        let replay = Ctx { replay: true, ..ctx(&w, false) };
        assert!(write(&w.m, &replay, "PUT", &format!("visits/{vid}"), Some(&edit)).unwrap().is_ok());
    }

    #[test]
    fn retried_creates_return_the_saved_record() {
        let mut w = world();
        let body = with_field(&student_body(&w, "Ana", "Reyes", "123456789013"), "id", json!(Uuid::new_v4()));
        save(&mut w, false, "POST", "students", Some(&body));
        let again = write(&w.m, &ctx(&w, false), "POST", "students", Some(&body)).unwrap().unwrap();
        assert!(again.changes.is_empty());
        assert_eq!(w.m.students.len(), 1);
    }

    #[test]
    fn name_search_matches_the_server() {
        let w = world();
        let s = MStudent {
            id: Uuid::new_v4(),
            lrn: "123456789012".into(),
            first_name: "Juan".into(),
            middle_name: Some("Santos".into()),
            last_name: "Dela Cruz".into(),
            gender: "male".into(),
            birthdate: NaiveDate::from_ymd_opt(2012, 1, 1).unwrap(),
            section_id: w.section,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        for (q, hit) in [("juan dela", true), ("Dela Cruz, Juan", true), ("cruz juan santos", true), ("cruz dela", false), ("santos dela", true), ("1234", true), ("2345", false), ("", true)] {
            assert_eq!(student_matches(&s, &words(Some(q))), hit, "{q:?}");
        }
    }

    #[test]
    fn ph_days_start_at_16_00_utc() {
        let (start, end) = ph_day_bounds(NaiveDate::from_ymd_opt(2026, 11, 20).unwrap());
        assert_eq!(start.to_rfc3339(), "2026-11-19T16:00:00+00:00");
        assert_eq!(end.to_rfc3339(), "2026-11-20T16:00:00+00:00");
    }
}
