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

use serde_json::Map;

use super::file::{Absent, FileError, HeaderMismatch, Row, absent_in};
use super::keep::Kept;
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

pub(crate) use super::problem::Problem;

/// One valid row: the Student it describes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Record {
    pub(crate) row: usize,
    pub(crate) code: String,
    pub(crate) fields: StudentFields,
    /// Roster Tag names, deduplicated, in file order.
    pub(crate) tags: Vec<String>,
    /// Fields whose column the row's sheet leaves out (#154). A listed
    /// Student keeps what is stored in them ([`super::keep::resolve`]).
    pub(crate) keep: Vec<Kept>,
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
#[cfg(test)]
pub(crate) fn validate(rows: &[Row], config: &DeploymentConfig) -> Result<Valid, Vec<Problem>> {
    validate_against(rows, config, &Taken::default())
}

/// [`validate`], also warning about an email an existing Student uses
/// (`taken`, see [`warnings::taken`]).
#[cfg(test)]
pub(crate) fn validate_against(
    rows: &[Row],
    config: &DeploymentConfig,
    taken: &Taken,
) -> Result<Valid, Vec<Problem>> {
    validate_in(rows, config, taken, &[])
}

/// Checks every row; `Err` holds every problem found, warnings included.
/// Warns about an email an existing Student uses (`taken`, see
/// [`warnings::taken`]). A sheet may leave out the may-be-empty columns
/// `absent` lists (#154): those cells are not checked, and the records
/// keep the stored values.
pub(crate) fn validate_in(
    rows: &[Row],
    config: &DeploymentConfig,
    taken: &Taken,
    absent: &[Absent],
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
        let gone = absent_in(absent, sheet);
        let has = |c: &str| !gone.iter().any(|g| g == c);
        let mut keep = Vec::new();
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
            // No column: the stored email stays, and a new Student has none.
            None if !has(&roster.email) => keep.push(Kept::Email),
            None => {
                empty_email += 1;
                sink.class(row, &roster.email, "missing_email");
            }
            Some(e) if !is_email(e) => sink.class(row, &roster.email, "invalid_email"),
            Some(e) => emails.add((records.len(), n, sheet), code, e, taken),
        }
        let locale = match &roster.locale {
            Some(l) => match row.get(&l.column) {
                None if !has(&l.column) => {
                    keep.push(Kept::Locale);
                    default_locale.clone()
                }
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
        if roster.tags.as_ref().is_some_and(|t| !has(&t.column)) {
            keep.push(Kept::Tags);
        } else if let Some(t) = &roster.tags {
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
            if !has(&spec.column) {
                keep.push(Kept::Attribute(key.to_string()));
                continue;
            }
            // An enum cell may name the value by its label (#81).
            let cell = row.get(&spec.column).unwrap_or_default();
            match spec.parse_import(cell, &config.default_locale) {
                Ok(Some(v)) => {
                    attributes.insert(key.to_string(), v);
                }
                Ok(None) if row.unreadable().any(|(h, _, _)| h == spec.column) => {}
                // A blank optional cell is fine (#134); a Deployment that
                // lists `empty_attribute` in `[roster] blocking` refuses it.
                Ok(None) if roster.blocks("empty_attribute") => blanks.add(sheet, &spec.column, n),
                Ok(None) => {}
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
            keep,
        });
    }
    let codes: BTreeSet<&str> = seen.keys().map(String::as_str).collect();
    let blocks = |class: &str| roster.blocks(class);
    // Latest row first, so an insert does not move the places still to come.
    for (index, row, sheet) in emails.shared(taken, &codes).into_iter().rev() {
        let r = &records[index];
        let p = Problem::new(row, &roster.email, "duplicate_email")
            .on(sheet)
            .about(&r.code, &r.fields.display_name)
            .showing(r.fields.email.as_deref().unwrap_or_default());
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
        self.push(echo(p, row, column));
    }

    /// A problem of a warning `class`: blocking only if the Deployment
    /// says so. It names its Student (#134).
    fn class(&mut self, row: &Row, column: &str, class: &str) {
        let p = Problem::new(row.number(), column, class).on(row.sheet());
        let blocks = self.roster.blocks(class);
        let get = |c: &str| row.get(c).unwrap_or_default();
        let p = p.about(
            get(&self.roster.student_code),
            get(&self.roster.display_name),
        );
        self.push(echo(p, row, column).with_severity(blocks));
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

/// `p` echoing the value of `row`'s cell under `column` (#154), if any.
fn echo(p: Problem, row: &Row, column: &str) -> Problem {
    match row.get(column) {
        Some(v) if !v.is_empty() => p.showing(v),
        _ => p,
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
        AttrError::OutOfRange => "out_of_range",
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
