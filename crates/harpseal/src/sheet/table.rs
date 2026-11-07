//! Finding the header row, the data and the footer of a [`Sheet`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::{Cell, HEADER_SEARCH_ROWS, RawRow, Row, Sheet, column_name};

/// Longest text kept for a skipped row, in characters.
const SKIPPED_TEXT_CHARS: usize = 200;

/// A test telling a key (a student id, a code) from anything else.
pub type KeyTest = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// What a table looks like: the columns its header must hold, its key
/// column, and whether a footer is skipped (see the module docs).
#[derive(Clone)]
pub struct TableSpec {
    columns: Vec<String>,
    key: Option<String>,
    is_key: KeyTest,
    /// `Some(numeric columns)` when footer rows are skipped.
    footer: Option<Vec<String>>,
}

impl std::fmt::Debug for TableSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TableSpec")
            .field("columns", &self.columns)
            .field("key", &self.key)
            .field("footer", &self.footer)
            .finish_non_exhaustive()
    }
}

impl TableSpec {
    /// A table whose header holds every one of `columns` (other headers
    /// may sit beside them). The key column is the first of them. Every
    /// row under the header is data: nothing is skipped unless
    /// [`TableSpec::skip_footer`] says so.
    pub fn new<I, S>(columns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            columns: columns.into_iter().map(Into::into).collect(),
            key: None,
            is_key: Arc::new(looks_like_key),
            footer: None,
        }
    }

    /// The key column, whose cell tells a data row from a footer row.
    pub fn key(mut self, column: impl Into<String>) -> Self {
        self.key = Some(column.into());
        self
    }

    /// What a key looks like (default [`looks_like_key`]: ids with
    /// digits). Supply your own for names or letter codes, e.g. a closure
    /// over the set of known codes, before turning the footer on.
    pub fn is_key(mut self, f: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        self.is_key = Arc::new(f);
        self
    }

    /// Skips totals and notes rows below the data. `numeric` names the
    /// columns where a totals row may hold values (amounts, counts); a
    /// value under any other required column keeps the row as data.
    pub fn skip_footer<I, S>(mut self, numeric: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.footer = Some(numeric.into_iter().map(Into::into).collect());
        self
    }

    fn key_column(&self) -> Option<&str> {
        self.key
            .as_deref()
            .or(self.columns.first().map(String::as_str))
    }
}

/// The default key test: no white space and at least one ASCII digit (a
/// student id or a code, never `Total`, `รวม` or a sentence).
pub fn looks_like_key(s: &str) -> bool {
    !s.chars().any(char::is_whitespace) && s.bytes().any(|b| b.is_ascii_digit())
}

/// A row a table left out, so callers can show it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SkippedRow {
    /// The sheet's name; empty for a CSV.
    pub sheet: String,
    /// The row number.
    pub row: usize,
    /// Its cells, joined by ` | ` (at most 200 characters).
    pub text: String,
}

/// A sheet's header and data rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    header_row: usize,
    headers: Vec<String>,
    pub(super) rows: Vec<Row>,
    footer: Vec<SkippedRow>,
    /// The footer rows' cells by header ([`Table::check_footer_total`]).
    pub(super) footer_rows: Vec<Row>,
}

impl Table {
    /// The header's row number.
    pub fn header_row(&self) -> usize {
        self.header_row
    }

    /// The header's cells by column (`""` for an empty one), trimmed.
    pub fn headers(&self) -> &[String] {
        &self.headers
    }

    /// The data rows, in sheet order.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The footer rows that were skipped (none unless the spec skips a
    /// footer). Show them: a skipped row is never silent.
    #[must_use = "skipped rows must be shown, never dropped silently"]
    pub fn footer(&self) -> &[SkippedRow] {
        &self.footer
    }

    /// The data rows and the skipped footer rows.
    #[must_use = "skipped rows must be shown, never dropped silently"]
    pub fn into_parts(self) -> (Vec<Row>, Vec<SkippedRow>) {
        (self.rows, self.footer)
    }
}

/// No usable header row among the first [`HEADER_SEARCH_ROWS`] non-empty
/// rows: none holds every column, or the one that does repeats one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HeaderError {
    /// The sheet's name.
    pub sheet: String,
    /// The header row, else the closest row (the one holding the most of
    /// the columns, else the first non-empty row); 0 for an empty sheet.
    pub row: usize,
    /// Its cells, trimmed (`""` for an empty one).
    pub headers: Vec<String>,
    /// The columns it lacks.
    pub missing: Vec<String>,
    /// The columns it holds more than once (which one is meant is
    /// unknown, so the sheet is not read).
    pub repeated: Vec<String>,
}

impl std::fmt::Display for HeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no usable header row in sheet `{}`", self.sheet)
    }
}

impl std::error::Error for HeaderError {}

/// Why [`Sheet::table`] gives no table.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TableError {
    /// No usable header row.
    Header(HeaderError),
    /// The header has rows under it, but the footer rule would skip them
    /// all: more likely data than a footer, so the sheet is not read.
    OnlyFooter {
        /// The sheet's name.
        sheet: String,
        /// The rows the footer rule matched.
        rows: Vec<SkippedRow>,
    },
}

impl TableError {
    /// The header error, if that is what it is.
    pub fn header(&self) -> Option<&HeaderError> {
        match self {
            Self::Header(e) => Some(e),
            Self::OnlyFooter { .. } => None,
        }
    }
}

impl std::fmt::Display for TableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Header(e) => e.fmt(f),
            Self::OnlyFooter { sheet, .. } => {
                write!(f, "only footer rows under the header in sheet `{sheet}`")
            }
        }
    }
}

impl std::error::Error for TableError {}

impl Sheet {
    /// The table `spec` describes: header, data rows and skipped footer.
    pub fn table(&self, spec: &TableSpec) -> Result<Table, TableError> {
        self.find(spec)
            .map_err(TableError::Header)
            .and_then(|(at, headers)| {
                let t = self.table_at(at, headers, spec);
                let numeric = spec.footer.as_deref().unwrap_or_default();
                // `รวม | 0` alone is an empty cohort, not swallowed data.
                if t.rows.is_empty() && super::total::footer_has_values(&t, numeric) {
                    return Err(TableError::OnlyFooter {
                        sheet: self.name.clone(),
                        rows: t.footer,
                    });
                }
                Ok(t)
            })
    }

    /// The header row's index among the rows, and its cells.
    fn find(&self, spec: &TableSpec) -> Result<(usize, Vec<String>), HeaderError> {
        let window = self.rows.iter().take(HEADER_SEARCH_ROWS).enumerate();
        let mut best: Option<(usize, usize)> = None;
        for (i, raw) in window {
            let headers = header_texts(raw);
            let found = spec.columns.iter().filter(|c| headers.contains(*c)).count();
            if found == spec.columns.len() {
                let repeated = repeated(&headers, &spec.columns);
                if !repeated.is_empty() {
                    return Err(self.error(raw, headers, Vec::new(), repeated));
                }
                return Ok((i, headers));
            }
            if found > 0 && best.is_none_or(|(_, f)| found > f) {
                best = Some((i, found));
            }
        }
        let Some(raw) = self.rows.get(best.map_or(0, |(i, _)| i)) else {
            return Err(HeaderError {
                sheet: self.name.clone(),
                row: 0,
                headers: Vec::new(),
                missing: spec.columns.clone(),
                repeated: Vec::new(),
            });
        };
        let headers = header_texts(raw);
        let missing = spec
            .columns
            .iter()
            .filter(|c| !headers.contains(c))
            .cloned()
            .collect();
        let repeated = repeated(&headers, &spec.columns);
        Err(self.error(raw, headers, missing, repeated))
    }

    fn error(
        &self,
        raw: &RawRow,
        headers: Vec<String>,
        missing: Vec<String>,
        repeated: Vec<String>,
    ) -> HeaderError {
        HeaderError {
            sheet: self.name.clone(),
            row: raw.number,
            headers,
            missing,
            repeated,
        }
    }

    fn table_at(&self, at: usize, headers: Vec<String>, spec: &TableSpec) -> Table {
        let body = &self.rows[at + 1..];
        let footer_len = match &spec.footer {
            Some(numeric) => body
                .iter()
                .rev()
                .take_while(|r| is_footer(r, &headers, spec, numeric))
                .count(),
            None => 0,
        };
        let (data, footer) = body.split_at(body.len() - footer_len);
        Table {
            header_row: self.rows[at].number,
            rows: data.iter().map(|r| self.row(r, &headers)).collect(),
            footer: footer.iter().map(|r| self.skipped(r)).collect(),
            footer_rows: footer.iter().map(|r| self.row(r, &headers)).collect(),
            headers,
        }
    }

    fn row(&self, raw: &RawRow, headers: &[String]) -> Row {
        let mut row = Row {
            sheet: self.name.clone(),
            number: raw.number,
            cells: BTreeMap::new(),
            unheaded: Vec::new(),
        };
        for (i, c) in raw.cells.iter().enumerate() {
            let Some(c) = c else { continue };
            // A header seen before (a repeated extra column) holds no
            // second value: the cell is reported as unheaded instead.
            let first = headers
                .get(i)
                .filter(|h| !h.is_empty())
                .filter(|h| headers.iter().position(|x| x == *h) == Some(i));
            match first {
                Some(h) => {
                    row.cells.insert(h.clone(), c.clone());
                }
                None => row.unheaded.push(column_name(i)),
            }
        }
        row
    }

    fn skipped(&self, raw: &RawRow) -> SkippedRow {
        let text = header_texts(raw)
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" | ");
        SkippedRow {
            sheet: self.name.clone(),
            row: raw.number,
            text: text.chars().take(SKIPPED_TEXT_CHARS).collect(),
        }
    }
}

/// Whether `r` is a footer row (see the module docs): a label in its
/// key cell, nothing under any other headed column but the numeric ones,
/// no unreadable cell and no cell outside the header.
fn is_footer(r: &RawRow, headers: &[String], spec: &TableSpec, numeric: &[String]) -> bool {
    let column = |name: &str| headers.iter().position(|h| h == name);
    let key = spec.key_column().and_then(column);
    let mut label = false;
    for (i, cell) in r.cells.iter().enumerate() {
        let Some(cell) = cell else { continue };
        let Cell::Text(text) = cell else {
            return false; // an unreadable cell is always reported
        };
        let header = headers.get(i).map(String::as_str).unwrap_or_default();
        if header.is_empty() {
            return false; // a cell outside the header is reported
        }
        if Some(i) == key {
            if (spec.is_key)(text) || is_malformed_key(text) {
                return false;
            }
            label = true;
        } else if !numeric.iter().any(|n| n == header) {
            return false; // a value under a non-numeric column: data
        }
    }
    // Only numbers and no label: a row whose id was lost, not a total.
    label
}

/// Digits and no letters (`6801 0002`, `68-01`): an id typed wrong, never
/// a label.
fn is_malformed_key(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit()) && !s.chars().any(char::is_alphabetic)
}

/// Columns of `columns` that `headers` holds more than once.
fn repeated(headers: &[String], columns: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for h in headers.iter().filter(|h| columns.contains(h)) {
        if !seen.insert(h) && !out.contains(h) {
            out.push(h.clone());
        }
    }
    out
}

/// A row's cells as header names: text trimmed, an error as its code.
fn header_texts(raw: &RawRow) -> Vec<String> {
    raw.cells
        .iter()
        .map(|c| match c {
            Some(Cell::Text(t)) => t.clone(),
            Some(Cell::Unreadable { code, .. }) => code.clone(),
            None => String::new(),
        })
        .collect()
}
