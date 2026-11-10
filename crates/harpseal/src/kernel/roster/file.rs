//! Reading a roster file into [`Row`]s (#8, #70) through the public
//! [`crate::sheet`] reader: CSV (UTF-8, BOM tolerated, `,` or `;`, named
//! `.csv`) or every worksheet of an XLSX (named `.xlsx`); any other name
//! (`.txt`, none) is refused, whatever it holds. In each non-empty sheet
//! the header row is found among the first rows (title rows above it are
//! skipped) and must hold the required `[roster]` columns exactly
//! (trimmed); a sheet without them is refused by name. A column that may
//! be empty may also be left out (#154): it is listed as [`Absent`], and
//! the import keeps what each listed Student has stored in it. Totals and
//! notes rows below the data (the reader's footer rule, with the
//! `integer` and `number` attribute columns as numeric) and hidden sheets
//! are skipped and listed ([`Skipped`]) for the preview and the report;
//! empty sheets and rows are skipped silently. Cells are trimmed and an
//! empty cell is absent. Pure: no I/O.

use serde::{Deserialize, Serialize};

use crate::config::{AttributeType, RosterConfig};
use crate::sheet::{HeaderError, Limits, SheetError, TableError, TableSpec, Workbook};

pub(crate) use crate::sheet::{Row, is_xlsx};
#[cfg(test)]
use {crate::sheet::column_name, crate::sheet::read::cell_text, calamine::Data};

/// Most data rows a file may hold, all sheets together.
pub(crate) const MAX_ROWS: usize = 20_000;
/// Most columns an XLSX sheet may use (cells further right are refused).
pub(crate) const MAX_COLUMNS: usize = 64;

/// Why a file cannot be read. Never carries cell contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileError {
    /// Named neither `.csv` nor `.xlsx`.
    Unsupported,
    /// A CSV that is not UTF-8.
    NotUtf8,
    /// Broken CSV quoting, on this row.
    Csv(usize),
    /// An XLSX calamine cannot open, or one without worksheets.
    Xlsx,
    /// No sheet with a non-empty cell.
    Empty,
    /// Content only in hidden sheets (named).
    OnlyHidden(Vec<String>),
    /// More than [`MAX_ROWS`] data rows (or an XLSX cell far below them).
    TooManyRows,
    /// An XLSX cell right of column [`MAX_COLUMNS`].
    TooManyColumns,
    /// More than 50 worksheets.
    TooManySheets,
    /// Sheets whose headers differ from the `[roster]` columns.
    Headers(Vec<HeaderMismatch>),
}

/// One sheet whose header row is missing or differs from `[roster]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeaderMismatch {
    /// The sheet; empty for a CSV.
    pub(crate) sheet: String,
    /// The header row, or the closest row to one.
    pub(crate) row: usize,
    pub(crate) missing: Vec<String>,
    pub(crate) unexpected: Vec<String>,
    /// A problem of the sheet as a whole, reported once instead of per
    /// column: `no_header_row` (no row holds any of the columns) or
    /// `only_footer` (the footer rule would skip every row under the
    /// header).
    pub(crate) whole: Option<&'static str>,
    /// The headers found on that row, in file order (#134); empty when no
    /// row holds any column (`no_header_row`) or for `only_footer`.
    pub(crate) found: Vec<String>,
}

/// The header row of one sheet of a refused file (#134), stored with the
/// failure so the page can set it beside the headers expected.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FoundHeaders {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) sheet: String,
    pub(crate) found: Vec<String>,
}

impl FileError {
    /// The header rows found in the sheets whose headers differ.
    pub(crate) fn found_headers(&self) -> Vec<FoundHeaders> {
        let FileError::Headers(sheets) = self else {
            return Vec::new();
        };
        sheets
            .iter()
            .filter(|m| !m.found.is_empty())
            .map(|m| FoundHeaders {
                sheet: m.sheet.clone(),
                found: m.found.clone(),
            })
            .collect()
    }
}

/// A row or sheet the import left out, shown on the preview and in the
/// report. `reason`: `footer` (a totals or notes row below the data) or
/// `hidden_sheet` (`row` 0, no text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Skipped {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) sheet: String,
    pub(crate) row: usize,
    pub(crate) reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) text: String,
}

/// A read file: its data rows and what was left out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Read {
    pub(crate) rows: Vec<Row>,
    pub(crate) skipped: Vec<Skipped>,
    /// The may-be-empty columns a sheet leaves out (#154).
    pub(crate) absent: Vec<Absent>,
}

/// The may-be-empty columns one sheet leaves out (#154), in template
/// order: its rows keep the stored values of these columns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Absent {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) sheet: String,
    pub(crate) columns: Vec<String>,
}

/// The columns `sheet` leaves out, per `absent`.
pub(crate) fn absent_in<'a>(absent: &'a [Absent], sheet: &str) -> &'a [String] {
    absent
        .iter()
        .find(|a| a.sheet == sheet)
        .map_or(&[], |a| a.columns.as_slice())
}

/// The headers a file must hold (#154): the student id, the name and the
/// attributes that may not be empty; the email when `[roster] blocking`
/// refuses a missing one, and every attribute when it refuses an empty
/// one (a column left out would otherwise slip past the rule). The others
/// may be left out.
pub(crate) fn required_headers(roster: &RosterConfig) -> Vec<String> {
    let mut v = vec![roster.student_code.clone(), roster.display_name.clone()];
    if roster.blocks("missing_email") {
        v.push(roster.email.clone());
    }
    let all = roster.blocks("empty_attribute");
    v.extend(
        roster
            .attributes_in_order()
            .into_iter()
            .filter(|(_, a)| !a.optional || all)
            .map(|(_, a)| a.column.clone()),
    );
    v
}

/// The headers a file may have (the template's), in template order:
/// student id, name, email, the Locale and Tags columns if mapped, then
/// the attributes in display order.
pub(crate) fn expected_headers(roster: &RosterConfig) -> Vec<String> {
    let mut v = vec![
        roster.student_code.clone(),
        roster.display_name.clone(),
        roster.email.clone(),
    ];
    v.extend(roster.locale.as_ref().map(|l| l.column.clone()));
    v.extend(roster.tags.as_ref().map(|t| t.column.clone()));
    v.extend(
        roster
            .attributes_in_order()
            .into_iter()
            .map(|(_, a)| a.column.clone()),
    );
    v
}

/// Reads every sheet into data rows, checking the headers against
/// `roster`; see [`read`].
#[cfg(test)]
pub(crate) fn parse(
    bytes: &[u8],
    file_name: &str,
    roster: &RosterConfig,
) -> Result<Vec<Row>, FileError> {
    read(bytes, file_name, roster).map(|r| r.rows)
}

/// The data rows of every visible sheet, and the skipped footer rows and
/// hidden sheets. Student ids are keys when they look like one
/// ([`crate::sheet::looks_like_key`]); a mistyped id is still data (and
/// reported).
pub(crate) fn read(
    bytes: &[u8],
    file_name: &str,
    roster: &RosterConfig,
) -> Result<Read, FileError> {
    let book = Workbook::open_with(bytes, file_name, Limits::new(MAX_ROWS, MAX_COLUMNS))
        .map_err(sheet_error)?;
    let expected = expected_headers(roster);
    let required = required_headers(roster);
    let numeric = roster
        .attributes
        .values()
        .filter(|a| matches!(a.kind, AttributeType::Integer | AttributeType::Number));
    let spec = TableSpec::new(required.clone())
        .key(roster.student_code.clone())
        .skip_footer(numeric.map(|a| a.column.clone()));
    let mut out = Read::default();
    let mut mismatches = Vec::new();
    let mut any = false;
    let id_only = TableSpec::new([roster.student_code.clone()]);
    let mut hidden = Vec::new();
    for s in book.sheets().iter().filter(|s| !s.is_empty()) {
        if s.is_hidden() {
            // A hidden sheet whose header holds the student id column (an
            // old cohort tab, even one missing another column) is listed
            // apart: a `full` import must not make its Students left unseen.
            let roster_sheet = s.table(&id_only).is_ok();
            hidden.push(s.name().to_string());
            out.skipped.push(Skipped {
                sheet: s.name().to_string(),
                row: 0,
                reason: if roster_sheet {
                    "hidden_roster_sheet"
                } else {
                    "hidden_sheet"
                }
                .into(),
                text: String::new(),
            });
            continue;
        }
        any = true;
        match s.table(&spec) {
            Ok(table) => {
                let unexpected = unexpected(table.headers(), &expected);
                if !unexpected.is_empty() {
                    mismatches.push(HeaderMismatch {
                        sheet: s.name().to_string(),
                        row: table.header_row(),
                        missing: Vec::new(),
                        unexpected,
                        whole: None,
                        found: table.headers().to_vec(),
                    });
                    continue;
                }
                let gone: Vec<String> = expected
                    .iter()
                    .filter(|h| !table.headers().contains(h))
                    .cloned()
                    .collect();
                if !gone.is_empty() {
                    out.absent.push(Absent {
                        sheet: s.name().to_string(),
                        columns: gone,
                    });
                }
                let (rows, footer) = table.into_parts();
                out.rows.extend(rows);
                out.skipped.extend(footer.into_iter().map(|f| Skipped {
                    sheet: f.sheet,
                    row: f.row,
                    reason: "footer".into(),
                    text: f.text,
                }));
                if out.rows.len() > MAX_ROWS {
                    return Err(FileError::TooManyRows);
                }
            }
            Err(TableError::Header(e)) => mismatches.push(not_found(e, &expected, &required)),
            Err(_) => mismatches.push(HeaderMismatch {
                sheet: s.name().to_string(),
                row: 0,
                missing: Vec::new(),
                unexpected: Vec::new(),
                whole: Some("only_footer"),
                found: Vec::new(),
            }),
        }
    }
    if !any && !hidden.is_empty() {
        return Err(FileError::OnlyHidden(hidden));
    }
    if !any {
        return Err(FileError::Empty);
    }
    if !mismatches.is_empty() {
        return Err(FileError::Headers(mismatches));
    }
    Ok(out)
}

fn sheet_error(e: SheetError) -> FileError {
    match e {
        SheetError::Unsupported => FileError::Unsupported,
        SheetError::NotUtf8 => FileError::NotUtf8,
        SheetError::Csv { row } => FileError::Csv(row),
        SheetError::TooManyRows => FileError::TooManyRows,
        SheetError::TooManyColumns => FileError::TooManyColumns,
        SheetError::TooManySheets => FileError::TooManySheets,
        _ => FileError::Xlsx,
    }
}

/// The closest row's mismatch (a repeated column is unexpected); when it
/// holds none of the columns, one "no header row" problem for the sheet.
fn not_found(e: HeaderError, expected: &[String], required: &[String]) -> HeaderMismatch {
    // No row holds a required column, nor one that may be left out.
    let no_header = e.missing.len() == required.len()
        && e.repeated.is_empty()
        && !e.headers.iter().any(|h| expected.contains(h));
    HeaderMismatch {
        unexpected: if no_header {
            Vec::new()
        } else {
            unexpected(&e.headers, expected)
        },
        missing: if no_header { Vec::new() } else { e.missing },
        // Nothing kept when no row holds any column (#134): the closest
        // row is then likely a Student's data, and the failure is kept for
        // good.
        found: if no_header {
            Vec::new()
        } else {
            e.headers.into_iter().filter(|h| !h.is_empty()).collect()
        },
        sheet: e.sheet,
        row: e.row,
        whole: no_header.then_some("no_header_row"),
    }
}

/// Headers outside `expected`, or repeated.
fn unexpected(header: &[String], expected: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    header
        .iter()
        .filter(|h| !h.is_empty())
        .filter(|h| !expected.contains(h) || !seen.insert(h.as_str()))
        .cloned()
        .collect()
}

/// The base name of an uploaded file, safe as a FileStore key segment:
/// no directories, no `\ : /` or control characters, at most 100
/// characters; `roster` when nothing is left.
pub(crate) fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, ':' | '"') {
                '_'
            } else {
                c
            }
        })
        .take(100)
        .collect();
    let clean = clean.trim().trim_start_matches('.').to_string();
    if clean.is_empty() {
        "roster".into()
    } else {
        clean
    }
}

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "sheets_tests.rs"]
mod sheets_tests;
