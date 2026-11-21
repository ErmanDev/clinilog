//! Field rules from the spec (§7a). The desktop form runs the same rules for
//! instant feedback; these are the ones that count.

use chrono::{Datelike, FixedOffset, NaiveDate, Utc};
use std::collections::BTreeMap;

use crate::error::AppError;

pub const NAME_MAX: usize = 50;
pub const MIN_AGE: i32 = 4;
pub const MAX_AGE: i32 = 25;

/// Collects per-field messages so the form can show them all at once.
#[derive(Default)]
pub struct FieldErrors(BTreeMap<String, String>);

impl FieldErrors {
    /// Records the first message for `field` (e.g. `"lrn"` or `"medicines.0.quantity"`).
    pub fn add(&mut self, field: impl Into<String>, msg: impl Into<String>) {
        self.0.entry(field.into()).or_insert_with(|| msg.into());
    }
    pub fn has(&self, field: &str) -> bool {
        self.0.contains_key(field)
    }
    /// `Ok(())` if empty, otherwise a 400 with every field message.
    pub fn finish(self) -> Result<(), AppError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(AppError::Validation(self.0))
        }
    }
}

/// Today's date in the Philippines (UTC+8), regardless of the server's time zone.
pub fn today_ph() -> NaiveDate {
    let ph = FixedOffset::east_opt(8 * 3600).expect("valid offset");
    Utc::now().with_timezone(&ph).date_naive()
}

/// Whole years between `birthdate` and `on`.
pub fn age_on(birthdate: NaiveDate, on: NaiveDate) -> i32 {
    let mut age = on.year() - birthdate.year();
    if (on.month(), on.day()) < (birthdate.month(), birthdate.day()) {
        age -= 1;
    }
    age
}

fn is_name_char(c: char) -> bool {
    c.is_alphabetic() || matches!(c, ' ' | '-' | '\'' | '\u{2019}' | '.')
}

/// First / middle / last name: 1–50 chars of letters (ñ and accents included),
/// spaces, hyphen, apostrophe or period, with at least one letter.
/// Returns the trimmed value.
pub fn name(errors: &mut FieldErrors, field: &'static str, label: &str, value: &str) -> String {
    let v = value.trim().to_string();
    let len = v.chars().count();
    if len == 0 {
        errors.add(field, format!("Enter the {label}."));
    } else if len > NAME_MAX {
        errors.add(field, format!("{} must be {NAME_MAX} characters or fewer.", capitalize(label)));
    } else if !v.chars().all(is_name_char) || !v.chars().any(char::is_alphabetic) {
        errors.add(
            field,
            format!("{} can only have letters, spaces, hyphens (-), apostrophes (') and periods (.).", capitalize(label)),
        );
    }
    v
}

/// Optional middle name: blank becomes `None`, otherwise the same rule as other names.
pub fn optional_name(errors: &mut FieldErrors, field: &'static str, label: &str, value: Option<&str>) -> Option<String> {
    match value.map(str::trim) {
        None | Some("") => None,
        Some(v) => Some(name(errors, field, label, v)),
    }
}

/// LRN: exactly 12 digits. Uniqueness is checked against the database separately.
pub fn lrn(errors: &mut FieldErrors, value: &str) -> String {
    let v = value.trim().to_string();
    if v.is_empty() {
        errors.add("lrn", "Enter the LRN.");
    } else if v.len() != 12 || !v.chars().all(|c| c.is_ascii_digit()) {
        errors.add("lrn", "LRN must be exactly 12 digits.");
    }
    v
}

pub fn gender(errors: &mut FieldErrors, value: &str) -> String {
    let v = value.trim().to_lowercase();
    if v != "male" && v != "female" {
        errors.add("gender", "Choose a gender.");
    }
    v
}

/// Birthdate: valid `YYYY-MM-DD`, not in the future, age 4–25 today.
pub fn birthdate(errors: &mut FieldErrors, value: &str, today: NaiveDate) -> Option<NaiveDate> {
    let v = value.trim();
    if v.is_empty() {
        errors.add("birthdate", "Enter the birthdate.");
        return None;
    }
    let Ok(d) = NaiveDate::parse_from_str(v, "%Y-%m-%d") else {
        errors.add("birthdate", "Enter a valid date.");
        return None;
    };
    if d > today {
        errors.add("birthdate", "Birthdate can't be in the future.");
    } else {
        let age = age_on(d, today);
        if !(MIN_AGE..=MAX_AGE).contains(&age) {
            errors.add("birthdate", format!("Age must be between {MIN_AGE} and {MAX_AGE} (this birthdate gives {age})."));
        }
    }
    Some(d)
}

/// Section name: 1–50 characters, no control characters.
pub fn section_name(errors: &mut FieldErrors, value: &str) -> String {
    let v = value.trim().to_string();
    let len = v.chars().count();
    if len == 0 {
        errors.add("section_name", "Enter the section name.");
    } else if len > 50 {
        errors.add("section_name", "Section name must be 50 characters or fewer.");
    } else if v.chars().any(char::is_control) {
        errors.add("section_name", "Section name has characters that aren't allowed.");
    }
    v
}

/// Required free text with a length range (after trimming), e.g. complaint 3–500.
pub fn text(errors: &mut FieldErrors, field: &'static str, label: &str, value: &str, min: usize, max: usize) -> String {
    let v = value.trim().to_string();
    let len = v.chars().count();
    if len == 0 {
        errors.add(field, format!("Enter the {label}."));
    } else if len < min {
        errors.add(field, format!("{} must be at least {min} characters.", capitalize(label)));
    } else if len > max {
        errors.add(field, format!("{} must be {max} characters or fewer.", capitalize(label)));
    } else if v.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        errors.add(field, format!("{} has characters that aren't allowed.", capitalize(label)));
    }
    v
}

/// Medicine quantity: a whole number from 1 to 100. Accepts any JSON value so
/// `2.5` or `"3"` get a clear message instead of a parse failure.
pub fn quantity(errors: &mut FieldErrors, field: String, value: &serde_json::Value) -> i32 {
    match value.as_i64() {
        Some(q) if (1..=100).contains(&q) => q as i32,
        Some(_) => {
            errors.add(field, "Quantity must be from 1 to 100.");
            0
        }
        None => {
            errors.add(field, "Quantity must be a whole number.");
            0
        }
    }
}

/// Username: 4–30 letters, digits or underscore. Returned trimmed (case kept for display;
/// uniqueness is checked case-insensitively).
pub fn username(errors: &mut FieldErrors, value: &str) -> String {
    let v = value.trim().to_string();
    if v.is_empty() {
        errors.add("username", "Enter a username.");
    } else if !(4..=30).contains(&v.len()) || !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        errors.add("username", "Username must be 4–30 characters: letters, numbers or underscore (_).");
    }
    v
}

/// Password: at least 8 characters with a letter and a number. Not trimmed; case-sensitive.
pub fn password(errors: &mut FieldErrors, field: &'static str, value: &str) {
    if value.chars().count() < 8 || !value.chars().any(char::is_alphabetic) || !value.chars().any(|c| c.is_ascii_digit()) {
        errors.add(field, "Password must be at least 8 characters, with at least one letter and one number.");
    } else if value.len() > 128 {
        errors.add(field, "Password must be 128 characters or fewer.");
    }
}

/// A person's full name for an account: same characters as student names, up to 100.
pub fn full_name(errors: &mut FieldErrors, value: &str) -> String {
    let v = value.trim().to_string();
    let len = v.chars().count();
    if len == 0 {
        errors.add("full_name", "Enter the full name.");
    } else if len > 100 {
        errors.add("full_name", "Full name must be 100 characters or fewer.");
    } else if !v.chars().all(is_name_char) || !v.chars().any(char::is_alphabetic) {
        errors.add("full_name", "Full name can only have letters, spaces, hyphens (-), apostrophes (') and periods (.).");
    }
    v
}

pub fn grade_level(errors: &mut FieldErrors, value: i32) -> i32 {
    if !(1..=12).contains(&value) {
        errors.add("grade_level", "Grade level must be from 1 to 12.");
    }
    value
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn names_allow_filipino_and_accented_letters() {
        for ok in ["Niño", "Peña", "Dela Cruz", "O'Neil", "Ma. Theresa", "Jean-Luc", "José", "D\u{2019}Souza"] {
            let mut e = FieldErrors::default();
            name(&mut e, "first_name", "first name", ok);
            assert!(e.0.is_empty(), "{ok} should be valid");
        }
    }

    #[test]
    fn names_reject_bad_input() {
        for bad in ["", "   ", "Juan2", "<script>", "Juan_", "..", &"a".repeat(51)] {
            let mut e = FieldErrors::default();
            name(&mut e, "first_name", "first name", bad);
            assert!(e.has("first_name"), "{bad:?} should be invalid");
        }
    }

    #[test]
    fn names_are_trimmed() {
        let mut e = FieldErrors::default();
        assert_eq!(name(&mut e, "f", "first name", "  Ana  "), "Ana");
        assert_eq!(optional_name(&mut e, "m", "middle name", Some("   ")), None);
        assert!(e.0.is_empty());
    }

    #[test]
    fn lrn_must_be_exactly_12_digits() {
        for (v, ok) in [("123456789012", true), (" 123456789012 ", true), ("12345678901", false), ("1234567890123", false), ("12345678901a", false), ("", false), ("１２３４５６７８９０１２", false)] {
            let mut e = FieldErrors::default();
            lrn(&mut e, v);
            assert_eq!(!e.has("lrn"), ok, "{v:?}");
        }
    }

    #[test]
    fn birthdate_rules() {
        let today = d("2026-11-20");
        let check = |v: &str| {
            let mut e = FieldErrors::default();
            birthdate(&mut e, v, today);
            !e.has("birthdate")
        };
        assert!(check("2012-05-01")); // 14
        assert!(check("2022-11-20")); // exactly 4 today
        assert!(!check("2022-11-21")); // 3, turns 4 tomorrow
        assert!(check("2000-11-21")); // 25, turns 26 tomorrow
        assert!(!check("2000-11-20")); // 26 today
        assert!(!check("2026-11-21")); // future
        assert!(!check("2012-02-30")); // not a date
        assert!(!check("05/01/2012")); // wrong format
        assert!(!check(""));
    }

    #[test]
    fn complaint_and_treatment_lengths() {
        let mut e = FieldErrors::default();
        assert_eq!(text(&mut e, "complaint", "complaint", "  Headache  ", 3, 500), "Headache");
        text(&mut e, "treatment", "treatment", "R", 1, 500);
        assert!(e.0.is_empty());
        for (v, min) in [("ab", 3), ("", 1), ("   ", 1)] {
            let mut e = FieldErrors::default();
            text(&mut e, "f", "field", v, min, 500);
            assert!(e.has("f"), "{v:?}");
        }
        let mut e = FieldErrors::default();
        text(&mut e, "f", "field", &"x".repeat(501), 1, 500);
        assert!(e.has("f"));
        let mut e = FieldErrors::default();
        text(&mut e, "f", "field", "line one\nline two", 1, 500);
        assert!(!e.has("f"), "newlines are allowed");
    }

    #[test]
    fn quantity_rules() {
        use serde_json::json;
        for (v, ok) in [(json!(1), true), (json!(100), true), (json!(0), false), (json!(101), false), (json!(-3), false), (json!(2.5), false), (json!("3"), false), (json!(null), false)] {
            let mut e = FieldErrors::default();
            quantity(&mut e, "q".into(), &v);
            assert_eq!(!e.has("q"), ok, "{v}");
        }
    }

    #[test]
    fn account_fields() {
        let check_user = |v: &str| {
            let mut e = FieldErrors::default();
            username(&mut e, v);
            !e.has("username")
        };
        assert!(check_user("nurse_joy") && check_user("  Admin1 ") && check_user("abcd"));
        assert!(!check_user("abc") && !check_user("nurse joy") && !check_user("niña") && !check_user(&"a".repeat(31)));

        let check_pw = |v: &str| {
            let mut e = FieldErrors::default();
            password(&mut e, "password", v);
            !e.has("password")
        };
        assert!(check_pw("Secret123") && check_pw("12345678a"));
        assert!(!check_pw("short1") && !check_pw("allletters") && !check_pw("12345678") && !check_pw(&format!("a1{}", "x".repeat(127))));

        let mut e = FieldErrors::default();
        assert_eq!(full_name(&mut e, " Ma. Lourdes Dela Peña "), "Ma. Lourdes Dela Peña");
        full_name(&mut e, "Nurse 2");
        assert!(e.has("full_name"));
    }

    #[test]
    fn age_handles_leap_day() {
        assert_eq!(age_on(d("2012-02-29"), d("2026-02-28")), 13);
        assert_eq!(age_on(d("2012-02-29"), d("2026-03-01")), 14);
    }
}
