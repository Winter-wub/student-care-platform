//! `roster_import` rows, their `counts` JSON, and the Students an import
//! compares the file with.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use super::validate::{Problem, Record};
use crate::message::Locale;
use crate::port_types::{Rows, Statement, Value};
use crate::students::{Merge, StudentFields, merge_import};
use crate::{Result, Services};

/// D1 allows 100 bound parameters per statement.
const PARAMS_PER_QUERY: usize = 90;

/// What happens to listed Students, counted per kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Tally {
    pub(crate) new: u64,
    pub(crate) updated: u64,
    pub(crate) unchanged: u64,
    /// Left Students listed again (they become active).
    pub(crate) returning: u64,
    /// Fields of listed Students kept because Staff edited them by hand
    /// (`Merge::skipped`). Fields, not Students: the preview and the
    /// result count the same thing.
    pub(crate) skipped_by_override: u64,
}

impl Tally {
    pub(crate) fn add(&mut self, other: &Tally) {
        self.new += other.new;
        self.updated += other.updated;
        self.unchanged += other.unchanged;
        self.returning += other.returning;
        self.skipped_by_override += other.skipped_by_override;
    }
}

/// The preview, computed when the file is uploaded.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Preview {
    #[serde(flatten)]
    pub(crate) tally: Tally,
    /// Active Students missing from a `full` file (0 in `upsert`).
    pub(crate) will_become_left: u64,
    /// Active Students missing from a `full` file whose status Staff set
    /// by hand: they stay active (0 in `upsert`).
    pub(crate) kept_by_status: u64,
    pub(crate) empty_email: u64,
    /// Active Students before the import.
    pub(crate) active_before: u64,
    /// Left > 20% of the active roster, or a `full` file shorter than it.
    pub(crate) warning: bool,
    /// Names of those who will become left, at most 200; empty in a
    /// `detailed` preview, whose leavers are in `diff::head_key`.
    pub(crate) left_names: Vec<String>,
    /// How many more will become left than `left_names` lists.
    pub(crate) left_more: u64,
    /// Whether the preview has per-Student lists (#134): the first
    /// changes and leavers in `diff::head_key`, every change in
    /// `diff::key`. `false` for a preview stored before them.
    pub(crate) detailed: bool,
    /// Live chat Links of everyone who will become left.
    pub(crate) links_cut: u64,
}

impl Preview {
    /// Listed Students the import changes: new, updated and returning.
    pub(crate) fn changed(&self) -> u64 {
        self.tally.new + self.tally.updated + self.tally.returning
    }
}

/// `roster_import.counts`. The file hash is `sha256` (a key ending in
/// `_hash` would be redacted from the audit row).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Counts {
    pub(crate) sha256: String,
    /// SHA-256 of the parsed rows (`rows.rs`); empty when none were stored.
    pub(crate) rows_sha256: String,
    /// When the stored file and its parsed rows are deleted (1 year after
    /// upload, `prune.rs`).
    pub(crate) keep_file_until: i64,
    pub(crate) preview: Preview,
    /// Rows and sheets the file left out (#70): footer rows and hidden
    /// sheets, shown on the preview and in the report.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) skipped: Vec<super::file::Skipped>,
    /// The may-be-empty columns the file leaves out (#154), per sheet:
    /// the preview says listed Students keep what is stored in them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) absent: Vec<super::file::Absent>,
    /// The warnings of a file that imports (#71), shown on the preview and
    /// in the report.
    #[serde(skip_serializing_if = "Warned::is_empty")]
    pub(crate) warnings: Warned,
    /// What the chunks applied so far.
    pub(crate) result: Tally,
    /// Students the last chunk marked left, Links it revoked, revoked
    /// notices that did not reach the Student.
    pub(crate) left: u64,
    /// Active Students the file did not list whose status Staff set by
    /// hand, counted when the import is done (`full` only).
    pub(crate) kept_by_status: u64,
    pub(crate) links_revoked: u64,
    pub(crate) notices_failed: u64,
    /// Left batches applied so far (the left phase's compare-and-set).
    pub(crate) left_batches: i64,
    /// The chunk run that last advanced `applied_rows` (its batch gate).
    pub(crate) run: String,
}

/// The warnings of a file that imports (#71).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Warned {
    /// Every warning found, also those not kept.
    pub(crate) total: u64,
    /// Warnings per class (reason code).
    pub(crate) classes: BTreeMap<String, u64>,
    /// The warnings kept, at most `MAX_PROBLEMS`, in file order.
    pub(crate) problems: Vec<Problem>,
}

impl Warned {
    pub(crate) fn new(warnings: Vec<Problem>) -> Self {
        Self {
            total: super::warnings::total(&warnings),
            classes: super::warnings::classes(&warnings),
            problems: super::warnings::kept(warnings),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.total == 0
    }
}

/// `roster_import.error`: a refused file or a failed Job.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Failure {
    /// The refused file's blocking problems, at most `MAX_PROBLEMS` rows.
    /// Its warnings are in `Counts::warnings`, as for a file that imports.
    pub(crate) problems: Vec<Problem>,
    /// Every blocking problem found, also those not kept.
    pub(crate) total: u64,
    /// Blocking problems per class (reason code) (#71).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) classes: BTreeMap<String, u64>,
    /// Why the Job failed (never file contents).
    pub(crate) job: Option<String>,
    /// The header row of each sheet whose headers differ (#134), so the
    /// page sets what the file has beside what it needs.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) headers: Vec<super::file::FoundHeaders>,
}

/// One `roster_import` row.
#[derive(Debug, Clone, Default)]
pub(crate) struct Import {
    pub(crate) id: String,
    pub(crate) staff: String,
    pub(crate) mode: String,
    pub(crate) state: String,
    pub(crate) file_key: String,
    pub(crate) file_name: String,
    pub(crate) total_rows: i64,
    pub(crate) applied_rows: i64,
    pub(crate) counts: Counts,
    pub(crate) failure: Option<Failure>,
    pub(crate) created_at: i64,
    pub(crate) started_at: Option<i64>,
    pub(crate) finished_at: Option<i64>,
}

const COLUMNS: &str = "r.id, r.mode, r.state, r.file_key, r.file_name, r.total_rows, \
    r.applied_rows, r.counts, r.error, r.created_at, r.started_at, r.finished_at, \
    s.display_name AS staff";

fn import_at(rows: &Rows, i: usize) -> Import {
    let t = |c: &str| text(rows, i, c).unwrap_or_default();
    let n = |c: &str| rows.get(i, c).and_then(Value::as_i64);
    Import {
        id: t("id"),
        staff: t("staff"),
        mode: t("mode"),
        state: t("state"),
        file_key: t("file_key"),
        file_name: t("file_name"),
        total_rows: n("total_rows").unwrap_or(0),
        applied_rows: n("applied_rows").unwrap_or(0),
        counts: serde_json::from_str(&t("counts")).unwrap_or_default(),
        failure: text(rows, i, "error").and_then(|e| serde_json::from_str(&e).ok()),
        created_at: n("created_at").unwrap_or(0),
        started_at: n("started_at"),
        finished_at: n("finished_at"),
    }
}

/// One import, `None` when there is no such id.
pub(crate) async fn load(s: &Services, id: &str) -> Result<Option<Import>> {
    let stmt = Statement::new(format!(
        "SELECT {COLUMNS} FROM roster_import r LEFT JOIN staff s ON s.id = r.staff_id \
         WHERE r.id = ?"
    ))
    .bind(id);
    let rows = s.db.query(stmt).await?;
    Ok((!rows.is_empty()).then(|| import_at(&rows, 0)))
}

/// The newest `n` imports.
pub(crate) async fn recent(s: &Services, n: i64) -> Result<Vec<Import>> {
    let stmt = Statement::new(format!(
        "SELECT {COLUMNS} FROM roster_import r LEFT JOIN staff s ON s.id = r.staff_id \
         ORDER BY r.created_at DESC, r.id DESC LIMIT ?"
    ))
    .bind(n);
    let rows = s.db.query(stmt).await?;
    Ok((0..rows.len()).map(|i| import_at(&rows, i)).collect())
}

/// The import holding the "one at a time" slot: `queued` / `running`, or
/// `previewed` and younger than `stale_before`.
pub(crate) async fn busy(s: &Services, stale_before: i64) -> Result<Option<Import>> {
    let stmt = Statement::new(format!(
        "SELECT {COLUMNS} FROM roster_import r LEFT JOIN staff s ON s.id = r.staff_id \
         WHERE {BUSY} ORDER BY r.created_at DESC LIMIT 1"
    ))
    .bind(stale_before);
    let rows = s.db.query(stmt).await?;
    Ok((!rows.is_empty()).then(|| import_at(&rows, 0)))
}

/// SQL condition on `r` (one parameter: the stale limit) for [`busy`].
pub(crate) const BUSY: &str = "(r.state IN ('queued', 'running') \
    OR (r.state = 'previewed' AND r.created_at >= ?))";

fn text(rows: &Rows, i: usize, c: &str) -> Option<String> {
    rows.get(i, c).and_then(Value::as_text).map(str::to_string)
}

/// A Student as stored, with what an import compares.
#[derive(Debug, Clone)]
pub(crate) struct Existing {
    pub(crate) id: String,
    pub(crate) fields: StudentFields,
    pub(crate) overrides: Vec<String>,
    /// `attributes` and `manual_overrides` as stored (compare-and-set).
    pub(crate) raw_overrides: String,
    /// Roster Tag names, sorted.
    pub(crate) roster_tags: Vec<String>,
}

const STUDENT: &str = "SELECT id, student_code, display_name, email, locale, status, \
    attributes, manual_overrides FROM student";
const ROSTER_TAGS: &str = "SELECT st.student_id, t.name FROM student_tag st \
    JOIN tag t ON t.id = st.tag_id WHERE st.source = 'roster'";

/// Every Student, by student code.
pub(crate) async fn all_students(s: &Services) -> Result<HashMap<String, Existing>> {
    let students = s.db.query(Statement::new(STUDENT)).await?;
    let tags = s.db.query(Statement::new(ROSTER_TAGS)).await?;
    Ok(collect(&students, &tags))
}

/// The Students with these codes, by code.
pub(crate) async fn students_by_code(
    s: &Services,
    codes: &[&str],
) -> Result<HashMap<String, Existing>> {
    let mut out = HashMap::new();
    for group in codes.chunks(PARAMS_PER_QUERY) {
        let marks = vec!["?"; group.len()].join(", ");
        let bind = |sql: String| {
            group
                .iter()
                .fold(Statement::new(sql), |st, c| st.bind(c.to_string()))
        };
        let students =
            s.db.query(bind(format!("{STUDENT} WHERE student_code IN ({marks})")))
                .await?;
        let tags =
            s.db.query(bind(format!(
                "{ROSTER_TAGS} AND st.student_id IN \
                 (SELECT id FROM student WHERE student_code IN ({marks}))"
            )))
            .await?;
        out.extend(collect(&students, &tags));
    }
    Ok(out)
}

fn collect(students: &Rows, tags: &Rows) -> HashMap<String, Existing> {
    let mut by_id: HashMap<String, Vec<String>> = HashMap::new();
    for i in 0..tags.len() {
        if let (Some(id), Some(name)) = (text(tags, i, "student_id"), text(tags, i, "name")) {
            by_id.entry(id).or_default().push(name);
        }
    }
    (0..students.len())
        .map(|i| {
            let t = |c: &str| text(students, i, c).unwrap_or_default();
            let attributes = match serde_json::from_str(&t("attributes")) {
                Ok(Json::Object(m)) => m,
                _ => serde_json::Map::new(),
            };
            let mut fields = StudentFields::new(
                t("display_name"),
                text(students, i, "email"),
                Locale(t("locale")),
            )
            .with_status(&t("status"));
            fields.attributes = attributes;
            let raw_overrides = t("manual_overrides");
            let mut roster_tags = by_id.remove(&t("id")).unwrap_or_default();
            roster_tags.sort();
            let e = Existing {
                id: t("id"),
                fields,
                overrides: serde_json::from_str(&raw_overrides).unwrap_or_default(),
                raw_overrides,
                roster_tags,
            };
            (t("student_code"), e)
        })
        .collect()
}

/// What a row does to its Student: the kind it counts as, the merged
/// fields and the fields an override kept.
pub(crate) fn classify(existing: Option<&Existing>, rec: &Record) -> (Tally, Merge) {
    let mut t = Tally::default();
    let Some(e) = existing else {
        t.new = 1;
        return (t, merge_import(&rec.fields, &rec.fields, &[]));
    };
    let m = merge_import(&e.fields, &rec.fields, &e.overrides);
    let mut tags = rec.tags.clone();
    tags.sort();
    t.skipped_by_override = m.skipped.len() as u64;
    if e.fields.status == "left" && m.fields.status == "active" {
        t.returning = 1;
    } else if same(&m.fields, &e.fields) && tags == e.roster_tags {
        t.unchanged = 1;
    } else {
        t.updated = 1;
    }
    (t, m)
}

/// Whether two Students' fields are the same, an attribute stored as JSON
/// `null` counting as absent (#134: no "updated" without a change).
fn same(a: &StudentFields, b: &StudentFields) -> bool {
    let set = |f: &StudentFields| {
        f.attributes
            .iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    // Every field by name: a new one is a compile error here, not a
    // silent "unchanged".
    let StudentFields {
        display_name,
        email,
        locale,
        status,
        attributes: _,
    } = a;
    *display_name == b.display_name
        && *email == b.email
        && *locale == b.locale
        && *status == b.status
        && set(a) == set(b)
}

/// Whether the Student's status is a manual override (kept by imports).
pub(crate) fn status_held(e: &Existing) -> bool {
    e.overrides.iter().any(|o| o == "status")
}
