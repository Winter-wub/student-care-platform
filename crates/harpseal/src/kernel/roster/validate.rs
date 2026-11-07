//! Row validation (#8): the whole file is refused on any blocking problem,
//! and the report lists every problem as `{row, column, reason}`. A
//! problem is a warning when its class is one of
//! `config::roster::WARNING_CLASSES` and the Deployment does not list it
//! in `[roster] blocking` (#71): the row imports and the warning is shown
//! in the preview. Pure: the `[roster]` mapping, `[student] id_pattern`
//! and the emails of existing Students come in as arguments.
//! Student codes follow the chat flow's rule (`id_pattern`, or 4 to 20
//! letters, digits or `-` when unset), so every imported Student can Link.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Map;

use super::file::{FileError, HeaderMismatch, Row};
use super::warnings::{self, Severity, Taken};
use crate::config::{AttrError, DeploymentConfig};
use crate::kernel::students::email::is_email;
use crate::message::Locale;
use crate::students::StudentFields;

/// Most problems kept in a report; the total is counted separately.
pub(crate) const MAX_PROBLEMS: usize = 1000;
/// Longest display name and Tag name, in characters.
const MAX_NAME_CHARS: usize = 200;
const MAX_TAG_CHARS: usize = 64;
/// Most roster Tags per Student (each is one bound parameter when the
/// Tags are attached; D1 allows 100 per statement).
pub(crate) const MAX_TAGS: usize = 20;
/// Characters that separate Tags in other files; refused in a Tags cell
/// unless they are the configured separator.
const TAG_SEPARATORS: [char; 3] = [',', ';', '|'];

/// One problem of a file. `reason` is a code; the text is
/// `kernel.roster.reason_<code>`. `row` 0 is the file (or the sheet) as a
/// whole. `sheet` names the XLSX sheet (empty for a CSV) and `cell` the
/// cell (`H2`) when one cell is at fault; both are left out of the stored
/// JSON when empty, so reports stored before #70 read unchanged. So is
/// `severity` (#71) when it is `blocking`, which every older problem is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Problem {
    pub(crate) row: usize,
    pub(crate) column: String,
    pub(crate) reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) sheet: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) cell: String,
    #[serde(default, skip_serializing_if = "Severity::is_blocking")]
    pub(crate) severity: Severity,
    /// Cells this problem stands for when it is one of several of a
    /// column (`empty_attribute`, #71); 0 for a problem of one place.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) count: u64,
    /// The first rows of such a problem.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) rows: Vec<usize>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl Problem {
    pub(crate) fn new(row: usize, column: &str, reason: &str) -> Self {
        Self {
            row,
            column: column.into(),
            reason: reason.into(),
            sheet: String::new(),
            cell: String::new(),
            severity: Severity::Blocking,
            count: 0,
            rows: Vec::new(),
        }
    }

    /// How many cells it counts as in a total: [`Problem::count`], else 1.
    pub(crate) fn weight(&self) -> u64 {
        self.count.max(1)
    }

    /// The same problem as a warning unless `blocks`.
    pub(crate) fn with_severity(mut self, blocks: bool) -> Self {
        self.severity = if blocks {
            Severity::Blocking
        } else {
            Severity::Warning
        };
        self
    }

    /// The same problem on `sheet`.
    pub(crate) fn on(mut self, sheet: &str) -> Self {
        self.sheet = sheet.into();
        self
    }
}

/// One valid row: the Student it describes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Record {
    pub(crate) row: usize,
    pub(crate) code: String,
    pub(crate) fields: StudentFields,
    /// Roster Tag names, deduplicated, in file order.
    pub(crate) tags: Vec<String>,
}

/// A valid file.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Valid {
    pub(crate) records: Vec<Record>,
    /// Rows without an email (allowed: such Students need a Manual Link).
    pub(crate) empty_email: usize,
    /// The warnings of the file (#71), in file order; none is blocking.
    pub(crate) warnings: Vec<Problem>,
}

/// The report of a file that cannot be read at all.
pub(crate) fn file_problems(e: &FileError) -> Vec<Problem> {
    let whole = |reason: &str| vec![Problem::new(0, "", reason)];
    match e {
        FileError::Unsupported => whole("unsupported"),
        FileError::NotUtf8 => whole("not_utf8"),
        FileError::Csv(r) => vec![Problem::new(*r, "", "bad_csv")],
        FileError::Xlsx => whole("bad_xlsx"),
        FileError::Empty => whole("empty"),
        FileError::OnlyHidden(sheets) => sheets
            .iter()
            .map(|s| Problem::new(0, "", "only_hidden").on(s))
            .collect(),
        FileError::TooManyRows => whole("too_many_rows"),
        FileError::TooManyColumns => whole("too_many_columns"),
        FileError::TooManySheets => whole("too_many_sheets"),
        FileError::Headers(sheets) => sheets.iter().flat_map(header_problems).collect(),
    }
}

fn header_problems(m: &HeaderMismatch) -> Vec<Problem> {
    if let Some(reason) = m.whole {
        return vec![Problem::new(0, "", reason).on(&m.sheet)];
    }
    let at = |h: &String, reason| Problem::new(m.row, h, reason).on(&m.sheet);
    m.missing
        .iter()
        .map(|h| at(h, "missing_header"))
        .chain(m.unexpected.iter().map(|h| at(h, "unexpected_header")))
        .collect()
}

/// Checks every row; `Err` holds every problem found, warnings included.
pub(crate) fn validate(rows: &[Row], config: &DeploymentConfig) -> Result<Valid, Vec<Problem>> {
    validate_against(rows, config, &Taken::default())
}

/// [`validate`], also warning about an email an existing Student uses
/// (`taken`, see [`warnings::taken`]).
pub(crate) fn validate_against(
    rows: &[Row],
    config: &DeploymentConfig,
    taken: &Taken,
) -> Result<Valid, Vec<Problem>> {
    let roster = &config.roster;
    let pattern = config.student.id_pattern.as_deref();
    let default_locale = roster
        .locale
        .as_ref()
        .and_then(|l| l.default.clone())
        .unwrap_or_else(|| config.default_locale.clone());
    let mut sink = Sink::new(roster);
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut records = Vec::with_capacity(rows.len());
    let mut emails = warnings::Emails::default();
    let mut blanks = warnings::Blanks::default();
    // Where each row's problems end in `sink.list`, to place the
    // duplicate-email warnings found after the loop in file order.
    let mut ends = Vec::with_capacity(rows.len());
    let mut empty_email = 0;
    for row in rows {
        let (n, sheet) = (row.number(), row.sheet());
        let before = sink.list.len();
        for column in row.unheaded() {
            sink.bad(row, column, "no_header");
        }
        let code = row.get(&roster.student_code).unwrap_or_default();
        if code.is_empty() {
            sink.bad(row, &roster.student_code, "required");
        } else if !crate::kernel::flow::fits(code, pattern) {
            sink.bad(row, &roster.student_code, "bad_code");
        } else if seen.insert(code.to_string(), n).is_some() {
            sink.bad(row, &roster.student_code, "duplicate_code");
        }
        let name = row.get(&roster.display_name).unwrap_or_default();
        if name.is_empty() {
            sink.bad(row, &roster.display_name, "required");
        } else if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
            sink.bad(row, &roster.display_name, "too_long");
        }
        let email = row.get(&roster.email);
        match email {
            None => {
                empty_email += 1;
                sink.class(row, &roster.email, "missing_email");
            }
            Some(e) if !is_email(e) => sink.class(row, &roster.email, "invalid_email"),
            Some(e) => emails.add((records.len(), n, sheet), code, e, taken),
        }
        let locale = match &roster.locale {
            Some(l) => match row.get(&l.column) {
                None => default_locale.clone(),
                Some(v) if is_locale(v) => Locale(v.to_string()),
                Some(_) => {
                    sink.bad(row, &l.column, "bad_locale");
                    default_locale.clone()
                }
            },
            None => default_locale.clone(),
        };
        let mut tags: Vec<String> = Vec::new();
        if let Some(t) = &roster.tags {
            let cell = row.get(&t.column).unwrap_or_default();
            let foreign = TAG_SEPARATORS
                .iter()
                .any(|c| cell.contains(*c) && !t.separator.contains(*c));
            if foreign {
                sink.bad(row, &t.column, "tag_separator");
            }
            for name in cell.split(t.separator.as_str()).map(str::trim) {
                if name.is_empty() || tags.iter().any(|x| x == name) {
                    continue;
                }
                if name.chars().count() > MAX_TAG_CHARS || name.chars().any(char::is_control) {
                    sink.bad(row, &t.column, "bad_tag");
                }
                tags.push(name.to_string());
            }
            if tags.len() > MAX_TAGS {
                sink.bad(row, &t.column, "too_many_tags");
            }
        }
        let mut attributes = Map::new();
        for (key, spec) in roster.attributes_in_order() {
            match spec.parse(row.get(&spec.column).unwrap_or_default()) {
                Ok(Some(v)) => {
                    attributes.insert(key.to_string(), v);
                }
                Ok(None) if row.unreadable().any(|(h, _, _)| h == spec.column) => {}
                Ok(None) => blanks.add(sheet, &spec.column, n),
                Err(e) => sink.bad(row, &spec.column, attr_reason(e)),
            }
        }
        unreadable(row, &mut sink.list, before);
        ends.push(sink.list.len());
        let mut fields =
            StudentFields::new(name, email.map(str::to_string), locale).with_status("active");
        fields.attributes = attributes;
        records.push(Record {
            row: n,
            code: code.to_string(),
            fields,
            tags,
        });
    }
    let codes: BTreeSet<&str> = seen.keys().map(String::as_str).collect();
    let blocks = |class: &str| roster.blocks(class);
    // Latest row first, so an insert does not move the places still to come.
    for (index, row, sheet) in emails.shared(taken, &codes).into_iter().rev() {
        let p = Problem::new(row, &roster.email, "duplicate_email").on(sheet);
        sink.list
            .insert(ends[index], p.with_severity(blocks("duplicate_email")));
    }
    sink.list.extend(blanks.problems(blocks("empty_attribute")));
    sink.finish(records, empty_email)
}

/// Where the problems of a file collect, and which of them block.
struct Sink<'a> {
    roster: &'a crate::config::RosterConfig,
    list: Vec<Problem>,
}

impl<'a> Sink<'a> {
    fn new(roster: &'a crate::config::RosterConfig) -> Self {
        Self {
            roster,
            list: Vec::new(),
        }
    }

    fn push(&mut self, p: Problem) {
        self.list.push(p);
    }

    /// A problem that always blocks.
    fn bad(&mut self, row: &Row, column: &str, reason: &str) {
        let p = Problem::new(row.number(), column, reason).on(row.sheet());
        self.push(p);
    }

    /// A problem of a warning `class`: blocking only if the Deployment says so.
    fn class(&mut self, row: &Row, column: &str, class: &str) {
        let p = Problem::new(row.number(), column, class).on(row.sheet());
        let blocks = self.roster.blocks(class);
        self.push(p.with_severity(blocks));
    }

    /// `Ok` with the warnings when nothing blocks, else every problem.
    fn finish(self, records: Vec<Record>, empty_email: usize) -> Result<Valid, Vec<Problem>> {
        if self.list.iter().any(|p| p.severity == Severity::Blocking) {
            return Err(self.list);
        }
        Ok(Valid {
            records,
            empty_email,
            warnings: self.list,
        })
    }
}

/// Reports each Excel error cell of `row` as `unreadable` (with its cell)
/// in place of what the empty value it reads as would have caused.
fn unreadable(row: &Row, problems: &mut Vec<Problem>, from: usize) {
    let cells: Vec<(&str, &str)> = row.unreadable().map(|(h, _, c)| (h, c)).collect();
    if cells.is_empty() {
        return;
    }
    let mut i = from;
    while i < problems.len() {
        if cells.iter().any(|(h, _)| problems[i].column == *h) {
            problems.remove(i);
        } else {
            i += 1;
        }
    }
    for (column, cell) in cells {
        let mut p = Problem::new(row.number(), column, "unreadable").on(row.sheet());
        p.cell = cell.into();
        problems.push(p);
    }
}

fn attr_reason(e: AttrError) -> &'static str {
    match e {
        AttrError::Required => "required",
        AttrError::NotInteger => "not_integer",
        AttrError::NotNumber => "not_number",
        AttrError::NotDate => "not_date",
        _ => "not_in_list",
    }
}

/// A language tag: 2 to 10 ASCII letters, digits or `-`, from a letter.
fn is_locale(s: &str) -> bool {
    (2..=10).contains(&s.len())
        && s.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

#[cfg(test)]
#[path = "validate_tests.rs"]
mod tests;
