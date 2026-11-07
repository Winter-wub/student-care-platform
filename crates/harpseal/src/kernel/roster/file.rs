//! Reading a roster file into [`Row`]s (#8): CSV (UTF-8, BOM tolerated,
//! `,` or `;`, named `.csv`) or the first sheet of an XLSX; a `.txt` is
//! refused even when it holds CSV. The first row holds the
//! headers, which must match the `[roster]` columns exactly (trimmed).
//! Empty rows are skipped, cells are trimmed and an empty cell is absent.
//! Pure: no I/O.

use std::collections::BTreeMap;
use std::io::Cursor;

use calamine::{Data, Reader, Xlsx};

use super::csv::{self, CsvError};
use crate::config::RosterConfig;

/// Most data rows a file may hold.
pub(crate) const MAX_ROWS: usize = 20_000;
/// Most columns an XLSX sheet may use (cells further right are refused).
pub(crate) const MAX_COLUMNS: usize = 64;

/// One data row: its row number in the file (the header row is 1), its
/// non-empty cells by header, and the columns (`A`, `B`, …) of non-empty
/// cells under an empty header or right of the last one, which
/// validation reports instead of dropping them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) number: usize,
    cells: BTreeMap<String, String>,
    pub(crate) unheaded: Vec<String>,
}

impl Row {
    /// The trimmed, non-empty cell under `column`.
    pub(crate) fn get(&self, column: &str) -> Option<&str> {
        self.cells.get(column).map(String::as_str)
    }
}

/// Why a file cannot be read. Never carries cell contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileError {
    /// Neither `.csv` nor `.xlsx` (nor an XLSX by its bytes).
    Unsupported,
    /// A CSV that is not UTF-8.
    NotUtf8,
    /// Broken CSV quoting.
    Csv(CsvError),
    /// An XLSX calamine cannot open, or one without sheets.
    Xlsx,
    /// No header row.
    Empty,
    /// More than [`MAX_ROWS`] data rows (or an XLSX cell below row
    /// `MAX_ROWS + 1`).
    TooManyRows,
    /// An XLSX cell right of column [`MAX_COLUMNS`].
    TooManyColumns,
    /// Headers that differ from the `[roster]` columns.
    Headers {
        missing: Vec<String>,
        unexpected: Vec<String>,
    },
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

/// Whether `bytes` named `file_name` is an XLSX (by extension, else by
/// its zip signature).
pub(crate) fn is_xlsx(bytes: &[u8], file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.ends_with(".xlsx") || (!lower.ends_with(".csv") && bytes.starts_with(b"PK\x03\x04"))
}

/// Reads the file into data rows, checking the headers against `roster`.
pub(crate) fn parse(
    bytes: &[u8],
    file_name: &str,
    roster: &RosterConfig,
) -> Result<Vec<Row>, FileError> {
    let lower = file_name.to_ascii_lowercase();
    let grid = if lower.ends_with(".txt") {
        return Err(FileError::Unsupported);
    } else if is_xlsx(bytes, file_name) {
        xlsx_grid(bytes)?
    } else if lower.ends_with(".csv") {
        let text = std::str::from_utf8(bytes).map_err(|_| FileError::NotUtf8)?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        csv::records(text, csv::delimiter(text))
            .map_err(FileError::Csv)?
            .into_iter()
            .enumerate()
            .map(|(i, r)| (i + 1, r))
            .collect()
    } else {
        return Err(FileError::Unsupported);
    };
    rows(grid, roster)
}

/// `(row number, cells)` per row, header row first.
type Grid = Vec<(usize, Vec<String>)>;

fn rows(grid: Grid, roster: &RosterConfig) -> Result<Vec<Row>, FileError> {
    let mut grid = grid
        .into_iter()
        .filter(|(_, cells)| cells.iter().any(|c| !c.trim().is_empty()));
    let (_, header) = grid.next().ok_or(FileError::Empty)?;
    let header: Vec<String> = header.iter().map(|h| h.trim().to_string()).collect();
    check_headers(&header, roster)?;
    let mut out = Vec::new();
    for (number, cells) in grid {
        if out.len() == MAX_ROWS {
            return Err(FileError::TooManyRows);
        }
        let mut row = Row {
            number,
            cells: BTreeMap::new(),
            unheaded: Vec::new(),
        };
        for (i, c) in cells.into_iter().enumerate() {
            let c = c.trim();
            match header.get(i).filter(|h| !h.is_empty()) {
                _ if c.is_empty() => {}
                Some(h) => {
                    row.cells.insert(h.clone(), c.to_string());
                }
                None => row.unheaded.push(column_name(i)),
            }
        }
        out.push(row);
    }
    Ok(out)
}

/// The spreadsheet name of 0-based column `i`: `A`, …, `Z`, `AA`, ….
pub(crate) fn column_name(i: usize) -> String {
    let mut n = i + 1;
    let mut out = Vec::new();
    while n > 0 {
        let d = (n - 1) % 26;
        out.push(b'A' + d as u8);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn check_headers(header: &[String], roster: &RosterConfig) -> Result<(), FileError> {
    let expected = expected_headers(roster);
    let mut seen = std::collections::BTreeSet::new();
    let mut unexpected = Vec::new();
    for h in header.iter().filter(|h| !h.is_empty()) {
        if !expected.contains(h) || !seen.insert(h.as_str()) {
            unexpected.push(h.clone());
        }
    }
    let missing: Vec<String> = expected
        .into_iter()
        .filter(|e| !header.contains(e))
        .collect();
    if missing.is_empty() && unexpected.is_empty() {
        Ok(())
    } else {
        Err(FileError::Headers {
            missing,
            unexpected,
        })
    }
}

/// Reads the first sheet cell by cell, never as calamine's dense range: a
/// sheet with one cell in A1 and one in XFD1048576 would ask for ~17e9
/// cells. A cell beyond row `MAX_ROWS + 1` or column [`MAX_COLUMNS`] refuses
/// the file before anything is allocated for it.
fn xlsx_grid(bytes: &[u8]) -> Result<Grid, FileError> {
    let mut book: Xlsx<_> =
        calamine::open_workbook_from_rs(Cursor::new(bytes)).map_err(|_| FileError::Xlsx)?;
    let first = book.sheet_names().first().cloned().ok_or(FileError::Xlsx)?;
    let mut cells = book
        .worksheet_cells_reader(&first)
        .map_err(|_| FileError::Xlsx)?;
    let mut rows: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    while let Some(cell) = cells.next_cell().map_err(|_| FileError::Xlsx)? {
        let (r, c) = cell.get_position();
        if r as usize > MAX_ROWS {
            return Err(FileError::TooManyRows);
        }
        if c as usize >= MAX_COLUMNS {
            return Err(FileError::TooManyColumns);
        }
        let text = cell_text(&Data::from(cell.get_value().clone()));
        if text.is_empty() {
            continue;
        }
        let row = rows.entry(r).or_default();
        if row.len() <= c as usize {
            row.resize(c as usize + 1, String::new());
        }
        row[c as usize] = text;
    }
    Ok(rows.into_iter().map(|(r, v)| (r as usize + 1, v)).collect())
}

/// A cell as text: integers without `.0`, dates as `YYYY-MM-DD` (with
/// ` HH:MM:SS` when the cell has a time of day).
pub(crate) fn cell_text(d: &Data) -> String {
    match d {
        Data::Int(i) => i.to_string(),
        Data::Float(f) => number_text(*f),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Data::DateTime(dt) if dt.is_datetime() => {
            let (y, m, d, h, mi, s, _) = dt.to_ymd_hms_milli();
            if (h, mi, s) == (0, 0, 0) {
                format!("{y:04}-{m:02}-{d:02}")
            } else {
                format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}")
            }
        }
        Data::DateTime(dt) => number_text(dt.as_f64()),
        Data::Error(e) => e.to_string(),
        Data::Empty => String::new(),
    }
}

fn number_text(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
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
