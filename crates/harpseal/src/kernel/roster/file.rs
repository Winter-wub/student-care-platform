//! Reading a roster file into [`Row`]s (#8, #70) through the public
//! [`crate::sheet`] reader: CSV (UTF-8, BOM tolerated, `,` or `;`, named
//! `.csv`) or every worksheet of an XLSX (named `.xlsx`); any other name
//! (`.txt`, none) is refused, whatever it holds. In each non-empty sheet
//! the header row is found among the first rows (title rows above it are
//! skipped) and must match the `[roster]` columns exactly (trimmed); a
//! sheet without it is refused by name. Totals and notes rows below the
//! data are skipped, empty sheets and rows too; cells are trimmed and an
//! empty cell is absent. Pure: no I/O.

use crate::config::RosterConfig;
use crate::sheet::{HeaderNotFound, Limits, SheetError, TableSpec, Workbook};

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
    /// More than [`MAX_ROWS`] data rows (or an XLSX cell far below them).
    TooManyRows,
    /// An XLSX cell right of column [`MAX_COLUMNS`].
    TooManyColumns,
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
    /// No row holds any of the columns: one problem, not one per column.
    pub(crate) no_header: bool,
}

/// The headers a file must have, in template order: student id, name,
/// email, the Locale and Tags columns if mapped, then the attributes in
/// display order.
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
/// `roster`. Student ids are keys when they look like one
/// ([`crate::sheet::looks_like_key`]): a mistyped id is still data (and reported),
/// a `รวม` or a note in the id column is a footer.
pub(crate) fn parse(
    bytes: &[u8],
    file_name: &str,
    roster: &RosterConfig,
) -> Result<Vec<Row>, FileError> {
    let book = Workbook::open_with(bytes, file_name, Limits::new(MAX_ROWS, MAX_COLUMNS))
        .map_err(sheet_error)?;
    let expected = expected_headers(roster);
    let spec = TableSpec::new(expected.clone()).key(roster.student_code.clone());
    let mut out = Vec::new();
    let mut mismatches = Vec::new();
    let mut any = false;
    for s in book.sheets().iter().filter(|s| !s.is_empty()) {
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
                        no_header: false,
                    });
                    continue;
                }
                out.extend(table.into_rows());
                if out.len() > MAX_ROWS {
                    return Err(FileError::TooManyRows);
                }
            }
            Err(e) => mismatches.push(not_found(e, &expected)),
        }
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
        _ => FileError::Xlsx,
    }
}

/// The closest row's mismatch; when it holds none of the columns, one
/// "no header row" problem for the sheet.
fn not_found(e: HeaderNotFound, expected: &[String]) -> HeaderMismatch {
    let no_header = e.missing.len() == expected.len();
    HeaderMismatch {
        unexpected: if no_header {
            Vec::new()
        } else {
            unexpected(&e.headers, expected)
        },
        missing: if no_header { Vec::new() } else { e.missing },
        sheet: e.sheet,
        row: e.row,
        no_header,
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
