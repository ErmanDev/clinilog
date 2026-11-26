//! The school's clinic records as kept on this PC for offline use.
//!
//! Two layers:
//! - the **base**: what the server last sent (`GET /sync`), saved encrypted in the cache;
//! - the **view**: the base with this PC's changes that haven't uploaded yet replayed on
//!   top (see `local.rs`). Pages read the view while offline, so a student added offline
//!   shows up in lists right away. A change the server refuses simply drops out of the view.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MSection {
    pub id: Uuid,
    pub grade_level: i32,
    pub section_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MMedicine {
    pub id: Uuid,
    pub name: String,
    pub unit: String,
}

/// Anyone who can appear as "attended by" on a visit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MStaff {
    pub id: Uuid,
    pub full_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MStudent {
    pub id: Uuid,
    pub lrn: String,
    pub first_name: String,
    pub middle_name: Option<String>,
    pub last_name: String,
    pub gender: String,
    pub birthdate: NaiveDate,
    pub section_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MLine {
    pub medicine_id: Uuid,
    pub quantity: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MVisit {
    pub id: Uuid,
    pub student_id: Uuid,
    /// The attending staff member.
    pub user_id: Uuid,
    pub visited_at: DateTime<Utc>,
    pub complaint: String,
    pub treatment: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub medicines: Vec<MLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Sections,
    Students,
    Medicines,
    Visits,
    Staff,
}

impl Table {
    pub const ALL: [Table; 5] = [Table::Sections, Table::Students, Table::Medicines, Table::Visits, Table::Staff];

    /// SQLite table holding the encrypted rows (fixed names only).
    pub fn sql_name(self) -> &'static str {
        match self {
            Table::Sections => "m_sections",
            Table::Students => "m_students",
            Table::Medicines => "m_medicines",
            Table::Visits => "m_visits",
            Table::Staff => "m_staff",
        }
    }
}

/// One row added, replaced or removed.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Section(MSection),
    Student(MStudent),
    Medicine(MMedicine),
    Visit(MVisit),
    Staff(MStaff),
    Delete(Table, Uuid),
}

impl Change {
    pub fn table(&self) -> Table {
        match self {
            Change::Section(_) => Table::Sections,
            Change::Student(_) => Table::Students,
            Change::Medicine(_) => Table::Medicines,
            Change::Visit(_) => Table::Visits,
            Change::Staff(_) => Table::Staff,
            Change::Delete(t, _) => *t,
        }
    }

    pub fn id(&self) -> Uuid {
        match self {
            Change::Section(r) => r.id,
            Change::Student(r) => r.id,
            Change::Medicine(r) => r.id,
            Change::Visit(r) => r.id,
            Change::Staff(r) => r.id,
            Change::Delete(_, id) => *id,
        }
    }

    /// The row as JSON for storage, or `None` for a delete.
    pub fn payload(&self) -> Option<serde_json::Value> {
        let v = match self {
            Change::Section(r) => serde_json::to_value(r),
            Change::Student(r) => serde_json::to_value(r),
            Change::Medicine(r) => serde_json::to_value(r),
            Change::Visit(r) => serde_json::to_value(r),
            Change::Staff(r) => serde_json::to_value(r),
            Change::Delete(..) => return None,
        };
        Some(v.expect("rows serialize"))
    }

    /// Rebuilds a stored row.
    pub fn from_payload(table: Table, value: serde_json::Value) -> Result<Change, serde_json::Error> {
        Ok(match table {
            Table::Sections => Change::Section(serde_json::from_value(value)?),
            Table::Students => Change::Student(serde_json::from_value(value)?),
            Table::Medicines => Change::Medicine(serde_json::from_value(value)?),
            Table::Visits => Change::Visit(serde_json::from_value(value)?),
            Table::Staff => Change::Staff(serde_json::from_value(value)?),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Mirror {
    pub sections: HashMap<Uuid, MSection>,
    pub students: HashMap<Uuid, MStudent>,
    pub medicines: HashMap<Uuid, MMedicine>,
    pub visits: HashMap<Uuid, MVisit>,
    pub staff: HashMap<Uuid, MStaff>,
}

impl Mirror {
    pub fn apply(&mut self, change: &Change) {
        match change {
            Change::Section(r) => {
                self.sections.insert(r.id, r.clone());
            }
            Change::Student(r) => {
                self.students.insert(r.id, r.clone());
            }
            Change::Medicine(r) => {
                self.medicines.insert(r.id, r.clone());
            }
            Change::Visit(r) => {
                self.visits.insert(r.id, r.clone());
            }
            Change::Staff(r) => {
                self.staff.insert(r.id, r.clone());
            }
            Change::Delete(t, id) => {
                match t {
                    Table::Sections => self.sections.remove(id).map(drop),
                    Table::Students => self.students.remove(id).map(drop),
                    Table::Medicines => self.medicines.remove(id).map(drop),
                    Table::Visits => self.visits.remove(id).map(drop),
                    Table::Staff => self.staff.remove(id).map(drop),
                };
            }
        }
    }

    pub fn apply_all<'a>(&mut self, changes: impl IntoIterator<Item = &'a Change>) {
        for c in changes {
            self.apply(c);
        }
    }

    /// Whether applying `change` would alter anything (skips rewriting identical rows).
    pub fn differs(&self, change: &Change) -> bool {
        match change {
            Change::Section(r) => self.sections.get(&r.id) != Some(r),
            Change::Student(r) => self.students.get(&r.id) != Some(r),
            Change::Medicine(r) => self.medicines.get(&r.id) != Some(r),
            Change::Visit(r) => self.visits.get(&r.id) != Some(r),
            Change::Staff(r) => self.staff.get(&r.id) != Some(r),
            Change::Delete(t, id) => match t {
                Table::Sections => self.sections.contains_key(id),
                Table::Students => self.students.contains_key(id),
                Table::Medicines => self.medicines.contains_key(id),
                Table::Visits => self.visits.contains_key(id),
                Table::Staff => self.staff.contains_key(id),
            },
        }
    }
}

// ---------- One page of `GET /sync` ----------

#[derive(Debug, Deserialize)]
pub struct Deletion {
    pub table_name: String,
    pub record_id: Uuid,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Cursors {
    pub students: Option<String>,
    pub visits: Option<String>,
    pub deleted: Option<String>,
}

/// An active account of the school, for offline login (Argon2 hash, never a password).
#[derive(Debug, Clone, Deserialize)]
pub struct SyncAccount {
    pub id: Uuid,
    pub full_name: String,
    pub username: String,
    pub role: String,
    pub password_hash: String,
}

#[derive(Debug, Deserialize)]
pub struct SyncPage {
    /// Missing from servers older than the account sync.
    #[serde(default)]
    pub school: Option<crate::api::SchoolInfo>,
    #[serde(default)]
    pub accounts: Option<Vec<SyncAccount>>,
    pub sections: Vec<MSection>,
    pub medicines: Vec<MMedicine>,
    pub staff: Vec<MStaff>,
    pub students: Vec<MStudent>,
    pub visits: Vec<MVisit>,
    pub deleted: Vec<Deletion>,
    pub next: Cursors,
    pub more: bool,
}

impl SyncPage {
    /// The changes that bring `base` up to date with this page. Sections, medicines and
    /// staff come in full, so anything missing from them was deleted on the server.
    pub fn changes(self, base: &Mirror) -> Vec<Change> {
        let mut out = Vec::new();
        fn replace<T: Clone>(
            out: &mut Vec<Change>,
            table: Table,
            current: impl Iterator<Item = Uuid>,
            incoming: &[T],
            id: impl Fn(&T) -> Uuid,
            wrap: impl Fn(T) -> Change,
        ) {
            let keep: std::collections::HashSet<Uuid> = incoming.iter().map(&id).collect();
            out.extend(current.filter(|i| !keep.contains(i)).map(|i| Change::Delete(table, i)));
            out.extend(incoming.iter().cloned().map(wrap));
        }
        replace(&mut out, Table::Sections, base.sections.keys().copied(), &self.sections, |r| r.id, Change::Section);
        replace(&mut out, Table::Medicines, base.medicines.keys().copied(), &self.medicines, |r| r.id, Change::Medicine);
        replace(&mut out, Table::Staff, base.staff.keys().copied(), &self.staff, |r| r.id, Change::Staff);
        out.extend(self.students.into_iter().map(Change::Student));
        out.extend(self.visits.into_iter().map(Change::Visit));
        for d in self.deleted {
            match d.table_name.as_str() {
                "students" => out.push(Change::Delete(Table::Students, d.record_id)),
                "clinic_visits" => out.push(Change::Delete(Table::Visits, d.record_id)),
                _ => {}
            }
        }
        out.retain(|c| base.differs(c));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(name: &str) -> MSection {
        MSection { id: Uuid::new_v4(), grade_level: 7, section_name: name.into() }
    }

    #[test]
    fn sync_page_replaces_reference_lists_and_applies_deletions() {
        let mut base = Mirror::default();
        let (keep, gone) = (section("Rosal"), section("Sampaguita"));
        base.apply(&Change::Section(keep.clone()));
        base.apply(&Change::Section(gone.clone()));
        let visit_id = Uuid::new_v4();
        base.visits.insert(
            visit_id,
            MVisit {
                id: visit_id,
                student_id: Uuid::new_v4(),
                user_id: Uuid::new_v4(),
                visited_at: Utc::now(),
                complaint: "Headache".into(),
                treatment: "Rest".into(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                medicines: vec![],
            },
        );
        let page: SyncPage = serde_json::from_value(serde_json::json!({
            "sections": [keep], "medicines": [], "staff": [], "students": [], "visits": [],
            "deleted": [{ "table_name": "clinic_visits", "record_id": visit_id, "deleted_at": "2026-11-26T00:00:00Z" }],
            "next": { "students": null, "visits": null, "deleted": "x" }, "more": false
        }))
        .unwrap();
        let changes = page.changes(&base);
        assert_eq!(changes.len(), 2, "unchanged section isn't rewritten: {changes:?}");
        base.apply_all(&changes);
        assert!(base.sections.contains_key(&keep.id) && !base.sections.contains_key(&gone.id));
        assert!(base.visits.is_empty());
    }

    #[test]
    fn rows_round_trip_through_storage() {
        let c = Change::Section(section("Rosal"));
        assert_eq!(Change::from_payload(Table::Sections, c.payload().unwrap()).unwrap(), c);
        assert!(Change::Delete(Table::Visits, Uuid::nil()).payload().is_none());
    }
}
