//! Finding the header row, the data and the footer of a [`Sheet`].

use std::collections::BTreeMap;

use super::{Cell, HEADER_SEARCH_ROWS, RawRow, Row, Sheet, column_name};

/// Most non-empty cells a footer row may have (a label and a value).
const FOOTER_CELLS: usize = 2;

/// What a table looks like: the columns its header must hold and its key
/// column (see the module docs for the footer rule).
#[derive(Debug, Clone)]
pub struct TableSpec {
    columns: Vec<String>,
    key: Option<String>,
    is_key: fn(&str) -> bool,
}

impl TableSpec {
    /// A table whose header holds every one of `columns` (other headers
    /// may sit beside them). The key column is the first of them.
    pub fn new<I, S>(columns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            columns: columns.into_iter().map(Into::into).collect(),
            key: None,
            is_key: looks_like_key,
        }
    }

    /// The key column, whose cell tells a data row from a footer row.
    pub fn key(mut self, column: impl Into<String>) -> Self {
        self.key = Some(column.into());
        self
    }

    /// What a key looks like (default [`looks_like_key`]).
    pub fn is_key(mut self, f: fn(&str) -> bool) -> Self {
        self.is_key = f;
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

/// A sheet's header and data rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    header_row: usize,
    headers: Vec<String>,
    rows: Vec<Row>,
    footer: Vec<usize>,
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

    /// Consumes the table into its data rows.
    pub fn into_rows(self) -> Vec<Row> {
        self.rows
    }

    /// Row numbers of the footer rows that were skipped.
    pub fn footer(&self) -> &[usize] {
        &self.footer
    }
}

/// No header row among the first [`HEADER_SEARCH_ROWS`] non-empty rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderNotFound {
    /// The sheet's name.
    pub sheet: String,
    /// The closest row: the one holding the most of the columns, else
    /// the first non-empty row; 0 for an empty sheet.
    pub row: usize,
    /// Its cells, trimmed (`""` for an empty one).
    pub headers: Vec<String>,
    /// The columns it lacks.
    pub missing: Vec<String>,
}

impl std::fmt::Display for HeaderNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no header row in sheet `{}`", self.sheet)
    }
}

impl std::error::Error for HeaderNotFound {}

impl Sheet {
    /// The table `spec` describes: header, data rows and skipped footer.
    pub fn table(&self, spec: &TableSpec) -> Result<Table, HeaderNotFound> {
        let window = self.rows.iter().take(HEADER_SEARCH_ROWS).enumerate();
        let mut best: Option<(usize, usize)> = None;
        for (i, raw) in window {
            let headers = header_texts(raw);
            let found = spec.columns.iter().filter(|c| headers.contains(*c)).count();
            if found == spec.columns.len() {
                return Ok(self.table_at(i, headers, spec));
            }
            if found > 0 && best.is_none_or(|(_, f)| found > f) {
                best = Some((i, found));
            }
        }
        let at = best.map_or(0, |(i, _)| i);
        let (row, headers) = self
            .rows
            .get(at)
            .map_or((0, Vec::new()), |r| (r.number, header_texts(r)));
        let missing = spec
            .columns
            .iter()
            .filter(|c| !headers.contains(c))
            .cloned()
            .collect();
        Err(HeaderNotFound {
            sheet: self.name.clone(),
            row,
            headers,
            missing,
        })
    }

    fn table_at(&self, at: usize, headers: Vec<String>, spec: &TableSpec) -> Table {
        let body = &self.rows[at + 1..];
        let key = spec
            .key_column()
            .and_then(|k| headers.iter().position(|h| h == k));
        let width = headers.len();
        let is_footer = |r: &RawRow| {
            let filled: Vec<usize> = (0..r.cells.len())
                .filter(|&i| r.cells[i].is_some())
                .collect();
            let keyed = key
                .and_then(|k| r.cells.get(k).cloned().flatten())
                .is_some_and(|c| matches!(c, Cell::Text(t) if (spec.is_key)(&t)));
            !keyed
                && filled.len() <= FOOTER_CELLS
                && filled.iter().all(|&i| i < width && !headers[i].is_empty())
        };
        let footer_len = body.iter().rev().take_while(|r| is_footer(r)).count();
        let (data, footer) = body.split_at(body.len() - footer_len);
        Table {
            header_row: self.rows[at].number,
            rows: data.iter().map(|r| self.row(r, &headers)).collect(),
            footer: footer.iter().map(|r| r.number).collect(),
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
            match headers.get(i).filter(|h| !h.is_empty()) {
                Some(h) => {
                    row.cells.insert(h.clone(), c.clone());
                }
                None => row.unheaded.push(column_name(i)),
            }
        }
        row
    }
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
