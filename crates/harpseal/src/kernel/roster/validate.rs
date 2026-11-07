//! Row validation (#8): the whole file is refused on any error, and the
//! report lists every problem as `{row, column, reason}`. Pure: the
//! `[roster]` mapping and `[student] id_pattern` come in as arguments.
//! Student codes follow the chat flow's rule (`id_pattern`, or 4 to 20
//! letters, digits or `-` when unset), so every imported Student can Link.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Map;

use super::file::{FileError, HeaderMismatch, Row};
use crate::config::{AttrError, DeploymentConfig};
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

/// One problem of a refused file. `reason` is a code; the text is
/// `kernel.roster.reason_<code>`. `row` 0 is the file (or the sheet) as a
/// whole. `sheet` names the XLSX sheet (empty for a CSV) and `cell` the
/// cell (`H2`) when one cell is at fault; both are left out of the stored
/// JSON when empty, so reports stored before #70 read unchanged. New
/// fields are added the same way (#71 adds the severity).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Problem {
    pub(crate) row: usize,
    pub(crate) column: String,
    pub(crate) reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) sheet: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) cell: String,
}

impl Problem {
    pub(crate) fn new(row: usize, column: &str, reason: &str) -> Self {
        Self {
            row,
            column: column.into(),
            reason: reason.into(),
            sheet: String::new(),
            cell: String::new(),
        }
    }

    fn on(mut self, sheet: &str) -> Self {
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
        FileError::TooManyRows => whole("too_many_rows"),
        FileError::TooManyColumns => whole("too_many_columns"),
        FileError::Headers(sheets) => sheets.iter().flat_map(header_problems).collect(),
    }
}

fn header_problems(m: &HeaderMismatch) -> Vec<Problem> {
    if m.no_header {
        return vec![Problem::new(0, "", "no_header_row").on(&m.sheet)];
    }
    let at = |h: &String, reason| Problem::new(m.row, h, reason).on(&m.sheet);
    m.missing
        .iter()
        .map(|h| at(h, "missing_header"))
        .chain(m.unexpected.iter().map(|h| at(h, "unexpected_header")))
        .collect()
}

/// Checks every row; `Err` holds every problem found.
pub(crate) fn validate(rows: &[Row], config: &DeploymentConfig) -> Result<Valid, Vec<Problem>> {
    let roster = &config.roster;
    let pattern = config.student.id_pattern.as_deref();
    let default_locale = roster
        .locale
        .as_ref()
        .and_then(|l| l.default.clone())
        .unwrap_or_else(|| config.default_locale.clone());
    let mut problems = Vec::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut records = Vec::with_capacity(rows.len());
    let mut empty_email = 0;
    for row in rows {
        let (n, sheet) = (row.number(), row.sheet());
        let before = problems.len();
        let mut bad =
            |column: &str, reason: &str| problems.push(Problem::new(n, column, reason).on(sheet));
        for column in row.unheaded() {
            bad(column, "no_header");
        }
        let code = row.get(&roster.student_code).unwrap_or_default();
        if code.is_empty() {
            bad(&roster.student_code, "required");
        } else if !crate::kernel::flow::fits(code, pattern) {
            bad(&roster.student_code, "bad_code");
        } else if seen.insert(code.to_string(), n).is_some() {
            bad(&roster.student_code, "duplicate_code");
        }
        let name = row.get(&roster.display_name).unwrap_or_default();
        if name.is_empty() {
            bad(&roster.display_name, "required");
        } else if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
            bad(&roster.display_name, "too_long");
        }
        let email = row.get(&roster.email);
        match email {
            None => empty_email += 1,
            Some(e) if !is_email(e) => bad(&roster.email, "bad_email"),
            Some(_) => {}
        }
        let locale = match &roster.locale {
            Some(l) => match row.get(&l.column) {
                None => default_locale.clone(),
                Some(v) if is_locale(v) => Locale(v.to_string()),
                Some(_) => {
                    bad(&l.column, "bad_locale");
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
                bad(&t.column, "tag_separator");
            }
            for name in cell.split(t.separator.as_str()).map(str::trim) {
                if name.is_empty() || tags.iter().any(|x| x == name) {
                    continue;
                }
                if name.chars().count() > MAX_TAG_CHARS || name.chars().any(char::is_control) {
                    bad(&t.column, "bad_tag");
                }
                tags.push(name.to_string());
            }
            if tags.len() > MAX_TAGS {
                bad(&t.column, "too_many_tags");
            }
        }
        let mut attributes = Map::new();
        for (key, spec) in roster.attributes_in_order() {
            match spec.parse(row.get(&spec.column).unwrap_or_default()) {
                Ok(Some(v)) => {
                    attributes.insert(key.to_string(), v);
                }
                Ok(None) => {}
                Err(e) => bad(&spec.column, attr_reason(e)),
            }
        }
        unreadable(row, &mut problems, before);
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
    if problems.is_empty() {
        Ok(Valid {
            records,
            empty_email,
        })
    } else {
        Err(problems)
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

/// A syntactically plausible address: one `@`, a non-empty local part of
/// printable ASCII without spaces (ASCII only: an address with Thai or
/// other non-ASCII characters, allowed by RFC 6531, is refused, because
/// the Link's email OTP goes through providers that may not accept it), a domain of dot-separated labels
/// (letters, digits, `-`, not at a label's ends) with a dot; at most 254
/// bytes.
pub(crate) fn is_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    let local_ok = !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .bytes()
            .all(|b| b.is_ascii_graphic() && !b"@\"(),:;<>[\\]".contains(&b));
    let label_ok = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    s.len() <= 254 && local_ok && domain.contains('.') && domain.split('.').all(label_ok)
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
