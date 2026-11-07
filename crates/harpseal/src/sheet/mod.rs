//! The spreadsheet reader the roster import uses (#70), public so a Module
//! reads its own uploads with the same rules.
//!
//! [`Workbook::open`] reads CSV (UTF-8, BOM tolerated, `,` or `;`, named
//! `.csv`) or XLSX (named `.xlsx`; told by the name only, any other name
//! is refused). A CSV is one [`Sheet`] with an empty name; an XLSX has one
//! per worksheet, hidden ones included and marked ([`Sheet::is_hidden`];
//! chart and macro sheets hold no cells and are left out). Cells are
//! trimmed; empty rows are dropped.
//!
//! [`Sheet::table`] then finds the header row and the data under it:
//!
//! - **Header**: the first of the first [`HEADER_SEARCH_ROWS`] non-empty
//!   rows that holds every column the [`TableSpec`] names (trimmed, exact).
//!   Title rows above it are skipped. When no row qualifies, or the one
//!   that does holds a column twice, [`HeaderError`] names the row (else
//!   the closest: the one holding the most columns, else the first) and
//!   what it lacks or repeats. A repeated header outside the spec keeps
//!   its first column; the cells of the others are [`Row::unheaded`].
//! - **Data**: every non-empty row under the header. Nothing is dropped
//!   unless the spec turns the footer on.
//! - **Footer** ([`TableSpec::skip_footer`], opt-in): read from the
//!   bottom up, a row is a footer row (totals, notes) when its key cell
//!   ([`TableSpec::key`], by default the first column) holds a **label**
//!   and every other headed column is empty except the numeric columns
//!   the spec names. A label is text that is not a key
//!   ([`TableSpec::is_key`], [`looks_like_key`] by default) and is not
//!   digits without letters (`6801 0002` is a malformed id). A row is
//!   always data when its key cell is empty (only numbers: an id was
//!   lost), when any other headed column holds a value (`,Pla,500`), or
//!   when it has an unreadable cell or a cell outside the header. The
//!   first row from the bottom that is not a footer row ends the footer.
//!   So with `Student id, Name, Amount` (Amount numeric), `Total,,22 500`
//!   and a note in column A are skipped. Skipped rows come back in
//!   [`Table::footer`], to be shown, never silently lost; when the rule
//!   would skip every row under the header, [`TableError::OnlyFooter`]
//!   is returned instead. The rule cannot tell `Pla Thong,,500` (a name
//!   typed in the id column) from `Total,,500`: a table keyed by names or
//!   letter codes needs its own `is_key` before the footer is turned on.
//!
//! Cell values: XLSX numbers as written without a trailing `.0`, dates as
//! `YYYY-MM-DD` (with ` HH:MM:SS` when they hold a time), booleans as
//! `TRUE` / `FALSE`, a formula as its cached value. An Excel error cell
//! (`#REF!`, `#N/A`, `#DIV/0!`, …) is [`Cell::Unreadable`] with its code
//! and cell reference, never text. Numbers stored as text go through
//! [`number`] or, exactly (money), [`number_scaled`].
//!
//! ```
//! use harpseal::sheet::{self, Blank, TableSpec, Workbook};
//!
//! let csv = "Seal Bay University fees 2569\n\
//!            Student id,Name,Amount\n\
//!            68010001,Nok Talay,\"12,500.50\"\n\
//!            68010002,Pla Thong,-\n\
//!            Total,,\"12,500.50\"\n";
//! let book = Workbook::open(csv.as_bytes(), "fees.csv").unwrap();
//! let spec = TableSpec::new(["Student id", "Name", "Amount"]).skip_footer(["Amount"]);
//! for s in book.sheets() {
//!     let table = s.table(&spec).unwrap();
//!     assert_eq!(table.header_row(), 2);
//!     assert_eq!(table.rows().len(), 2, "the totals row is skipped…");
//!     assert_eq!(table.footer()[0].row, 5, "…and reported");
//!     let first = &table.rows()[0];
//!     assert_eq!((first.sheet(), first.number()), ("", 3));
//!     let satang = sheet::number_scaled(first.get("Amount").unwrap_or(""), 2, Blank::Missing);
//!     assert_eq!(satang, Ok(Some(1_250_050)));
//!     let dash = sheet::number(table.rows()[1].get("Amount").unwrap_or(""), Blank::Zero);
//!     assert_eq!(dash, Ok(Some(0.0)));
//! }
//! ```
//!
//! Pure: no I/O.

pub(crate) mod csv;
mod number;
pub(crate) mod read;
mod table;

use std::collections::BTreeMap;

pub use number::{Blank, MAX_F64_DIGITS, MAX_SCALE, NotANumber, number, number_scaled};
pub use table::{HeaderError, KeyTest, SkippedRow, Table, TableError, TableSpec, looks_like_key};

/// Rows searched for the header, counting only non-empty ones.
pub const HEADER_SEARCH_ROWS: usize = 10;

/// Why a file cannot be read. Never carries cell contents.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetError {
    /// Named neither `.csv` nor `.xlsx`.
    Unsupported,
    /// A CSV that is not UTF-8.
    NotUtf8,
    /// Broken CSV quoting; the record starts on this row.
    Csv {
        /// 1-based row of the broken record.
        row: usize,
    },
    /// An XLSX that cannot be opened.
    Xlsx,
    /// More rows than [`Limits::max_rows`] allows.
    TooManyRows,
    /// A cell right of column [`Limits::max_columns`].
    TooManyColumns,
    /// More worksheets than [`Limits::max_sheets`].
    TooManySheets,
}

impl std::fmt::Display for SheetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("not a .csv or .xlsx file"),
            Self::NotUtf8 => f.write_str("the CSV is not UTF-8"),
            Self::Csv { row } => write!(f, "broken CSV quoting on row {row}"),
            Self::Xlsx => f.write_str("the XLSX cannot be opened"),
            Self::TooManyRows => f.write_str("too many rows"),
            Self::TooManyColumns => f.write_str("too many columns"),
            Self::TooManySheets => f.write_str("too many sheets"),
        }
    }
}

impl std::error::Error for SheetError {}

/// Size guards, checked while reading: an XLSX is read cell by cell, so a
/// sheet with one cell in A1 and one in XFD1048576 is refused before
/// anything is allocated for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// Most data rows. A sheet may hold [`Limits::SLACK`] more rows (titles,
    /// header, footer); the whole workbook `SLACK` more per sheet.
    pub max_rows: usize,
    /// Most columns (cells further right are refused).
    pub max_columns: usize,
    /// Most worksheets, counted before any is read (default 50).
    pub max_sheets: usize,
}

impl Limits {
    /// Rows a sheet may hold beyond `max_rows`.
    pub const SLACK: usize = HEADER_SEARCH_ROWS + 10;

    /// Limits of `max_rows` data rows and `max_columns` columns, and the
    /// default 50 sheets.
    pub fn new(max_rows: usize, max_columns: usize) -> Self {
        Self {
            max_rows,
            max_columns,
            max_sheets: 50,
        }
    }

    /// The same limits with at most `n` worksheets.
    pub fn max_sheets(mut self, n: usize) -> Self {
        self.max_sheets = n;
        self
    }
}

impl Default for Limits {
    /// The roster's: 20 000 rows, 64 columns, 50 sheets.
    fn default() -> Self {
        Self::new(20_000, 64)
    }
}

/// Whether `file_name` names an XLSX: by its extension only.
pub fn is_xlsx(file_name: &str) -> bool {
    file_name.to_ascii_lowercase().ends_with(".xlsx")
}

/// A read file: its sheets in workbook order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workbook {
    sheets: Vec<Sheet>,
}

impl Workbook {
    /// Reads `bytes` as the file `file_name` names, with [`Limits::default`].
    pub fn open(bytes: &[u8], file_name: &str) -> Result<Self, SheetError> {
        Self::open_with(bytes, file_name, Limits::default())
    }

    /// Reads `bytes` as the file `file_name` names, within `limits`.
    pub fn open_with(bytes: &[u8], file_name: &str, limits: Limits) -> Result<Self, SheetError> {
        let sheets = if is_xlsx(file_name) {
            read::xlsx(bytes, limits)?
        } else if file_name.to_ascii_lowercase().ends_with(".csv") {
            vec![read::csv(bytes, limits)?]
        } else {
            return Err(SheetError::Unsupported);
        };
        Ok(Self { sheets })
    }

    /// The sheets, in workbook order (empty ones included).
    pub fn sheets(&self) -> &[Sheet] {
        &self.sheets
    }
}

/// One sheet: its name (empty for a CSV) and its non-empty rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sheet {
    name: String,
    rows: Vec<RawRow>,
    hidden: bool,
}

/// A non-empty row as read: its 1-based number and its cells by column.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawRow {
    number: usize,
    cells: Vec<Option<Cell>>,
}

impl Sheet {
    /// The sheet's name; empty for a CSV.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the XLSX hides the sheet (hidden or very hidden). Staff do
    /// not see it in Excel: callers usually skip it and say so.
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// Whether the sheet has no non-empty cell.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// A non-empty cell.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Cell {
    /// Its trimmed text.
    Text(String),
    /// An Excel error cell.
    Unreadable {
        /// The error, e.g. `#REF!`.
        code: String,
        /// The cell, e.g. `H2`.
        cell: String,
    },
}

/// One data row: its sheet, its 1-based row number in the sheet, its
/// non-empty cells by header, and the columns (`A`, `B`, …) of non-empty
/// cells under an empty header or right of the last one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    sheet: String,
    pub(crate) number: usize,
    cells: BTreeMap<String, Cell>,
    pub(crate) unheaded: Vec<String>,
}

impl Row {
    /// The sheet's name; empty for a CSV.
    pub fn sheet(&self) -> &str {
        &self.sheet
    }

    /// The row number in the sheet (1-based, as a spreadsheet shows it).
    pub fn number(&self) -> usize {
        self.number
    }

    /// The text under `column`; `None` when empty or unreadable.
    pub fn get(&self, column: &str) -> Option<&str> {
        match self.cells.get(column) {
            Some(Cell::Text(t)) => Some(t),
            _ => None,
        }
    }

    /// The cell under `column`; `None` when empty.
    pub fn cell(&self, column: &str) -> Option<&Cell> {
        self.cells.get(column)
    }

    /// The unreadable cells: `(header, code, cell reference)`.
    pub fn unreadable(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.cells.iter().filter_map(|(h, c)| match c {
            Cell::Unreadable { code, cell } => Some((h.as_str(), code.as_str(), cell.as_str())),
            Cell::Text(_) => None,
        })
    }

    /// Columns (`A`, `B`, …) of non-empty cells no header names.
    pub fn unheaded(&self) -> &[String] {
        &self.unheaded
    }
}

/// The spreadsheet name of 0-based column `i`: `A`, …, `Z`, `AA`, ….
pub fn column_name(i: usize) -> String {
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

#[cfg(test)]
mod tests;
