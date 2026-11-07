//! "Download all data" (#78): every table a backup holds, Kernel and
//! Module tables alike, as one XLSX workbook with one sheet per table,
//! written by `rust_xlsxwriter`. Secret columns are left out
//! ([`is_secret`]); everything else is exported as stored.
//!
//! The workbook is built in memory in one request (`rust_xlsxwriter` has
//! no streaming mode on wasm32), at about 200 bytes per cell, so the page
//! first counts the cells ([`cells`]) and refuses above `[backup]
//! max_export_cells`. A table longer than Excel's 1,048,575 data rows is
//! cut there and named in [`Export::cut`].

use rust_xlsxwriter::{Format, Workbook, XlsxError};

use super::pages::read_pages;
use super::schema;
use crate::port_types::Value;
use crate::{Deployment, Error, Result};

/// Data rows per sheet: Excel's limit less the header row.
const MAX_ROWS: u32 = 1_048_575;
/// Longest text in one cell (Excel's limit).
const MAX_CELL_CHARS: usize = 32_767;
/// Longest sheet name (Excel's limit).
const MAX_SHEET_CHARS: usize = 31;
/// Integers above this lose digits as an Excel number: written as text.
const MAX_EXACT: i64 = 1 << 53;

/// A built workbook and what went into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Export {
    /// The `.xlsx` bytes.
    pub(crate) bytes: Vec<u8>,
    /// Sheets written (one per table).
    pub(crate) tables: usize,
    /// Data rows written in all.
    pub(crate) rows: u64,
    /// `table.column` of every column left out as a secret.
    pub(crate) excluded: Vec<String>,
    /// Tables cut at [`MAX_ROWS`].
    pub(crate) cut: Vec<String>,
}

/// Whether `table.column` holds a secret and stays out of the export:
/// password hashes (`password`, `password_*`), any `*_hash` or `hash`
/// (OTP and challenge hashes), anything naming a `token`, `salt` or
/// `secret`, and the `id` of a `*_session` table (the hash of the session
/// cookie). Module tables follow the same rule: name secret columns so.
pub(crate) fn is_secret(table: &str, column: &str) -> bool {
    let c = column.to_ascii_lowercase();
    c.starts_with("password")
        || c == "hash"
        || c.ends_with("_hash")
        || ["token", "salt", "secret"].iter().any(|w| c.contains(w))
        || (table.ends_with("_session") && c == "id")
}

fn xlsx(e: XlsxError) -> Error {
    Error::Invalid(format!("xlsx: {e}"))
}

/// `name` as a sheet name: at most 31 characters, none of `[]:*?/\`,
/// unique among `taken` (a `~2`, `~3`… suffix).
fn sheet_name(name: &str, taken: &[String]) -> String {
    let clean: String = name
        .chars()
        .map(|c| if "[]:*?/\\'".contains(c) { '_' } else { c })
        .collect();
    let base: String = clean.chars().take(MAX_SHEET_CHARS).collect();
    let mut candidate = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&candidate)) {
        let tail = format!("~{n}");
        let keep = MAX_SHEET_CHARS - tail.chars().count();
        candidate = format!("{}{tail}", base.chars().take(keep).collect::<String>());
        n += 1;
    }
    candidate
}

/// Text cut to what one cell holds.
fn cell_text(s: &str) -> std::borrow::Cow<'_, str> {
    if s.chars().count() <= MAX_CELL_CHARS {
        return s.into();
    }
    let mut t: String = s.chars().take(MAX_CELL_CHARS - 1).collect();
    t.push('…');
    t.into()
}

/// Cells the workbook of `d` would hold (rows × exported columns, every
/// table, header rows left out), counted without reading the rows.
pub(crate) async fn cells(d: &Deployment) -> Result<u64> {
    let db = d.services().db.as_ref();
    let mut total = 0u64;
    for t in schema::tables(db).await? {
        let kept = t
            .columns
            .iter()
            .filter(|c| !is_secret(&t.name, &c.name))
            .count();
        total += schema::count(db, &t.name).await? * kept as u64;
    }
    Ok(total)
}

/// Builds the workbook of every table of `d` (the backup's table list, in
/// name order), secret columns left out.
pub(crate) async fn export(d: &Deployment) -> Result<Export> {
    let db = d.services().db.as_ref();
    let now = d.services().now_ms();
    let tables = schema::tables(db).await?;
    let mut book = Workbook::new();
    let bold = Format::new().set_bold();
    let mut names = Vec::with_capacity(tables.len());
    let (mut rows, mut excluded, mut cut) = (0u64, Vec::new(), Vec::new());
    for t in &tables {
        // Indexes into each page row (after the rowid), kept columns only.
        let kept: Vec<usize> = (0..t.columns.len())
            .filter(|&i| !is_secret(&t.name, &t.columns[i].name))
            .collect();
        excluded.extend(
            t.columns
                .iter()
                .filter(|c| is_secret(&t.name, &c.name))
                .map(|c| format!("{}.{}", t.name, c.name)),
        );
        let name = sheet_name(&t.name, &names);
        let sheet = book.add_worksheet();
        sheet.set_name(&name).map_err(xlsx)?;
        names.push(name);
        for (col, &i) in kept.iter().enumerate() {
            let col = u16::try_from(col).map_err(|_| Error::Invalid("too many columns".into()))?;
            sheet
                .write_string_with_format(0, col, &t.columns[i].name, &bold)
                .map_err(xlsx)?;
        }
        sheet.set_freeze_panes(1, 0).map_err(xlsx)?;
        let mut row = 0u32;
        let mut full = false;
        read_pages(db, t, now, |page, skip| {
            for values in &page.rows {
                if row >= MAX_ROWS {
                    full = true;
                    return Ok(false);
                }
                row += 1;
                for (col, &i) in kept.iter().enumerate() {
                    let col = col as u16;
                    match values.get(skip + i) {
                        None | Some(Value::Null) => continue,
                        Some(Value::Integer(n)) if n.abs() <= MAX_EXACT => {
                            sheet.write_number(row, col, *n as f64)
                        }
                        Some(Value::Integer(n)) => sheet.write_string(row, col, n.to_string()),
                        Some(Value::Real(x)) => sheet.write_number(row, col, *x),
                        Some(Value::Text(s)) => sheet.write_string(row, col, cell_text(s)),
                        Some(Value::Blob(b)) => {
                            sheet.write_string(row, col, format!("[{} bytes]", b.len()))
                        }
                    }
                    .map_err(xlsx)?;
                }
            }
            Ok(true)
        })
        .await?;
        if full {
            cut.push(t.name.clone());
        }
        rows += u64::from(row);
    }
    let bytes = book.save_to_buffer().map_err(xlsx)?;
    Ok(Export {
        bytes,
        tables: tables.len(),
        rows,
        excluded,
        cut,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_named_by_the_rule() {
        for (t, c) in [
            ("staff", "password_hash"),
            ("verification_challenge", "secret_hash"),
            ("job", "lock_token"),
            ("menu_salt", "salt"),
            ("staff_session", "id"),
            ("app_session", "id"),
            ("fees_portal", "api_secret"),
        ] {
            assert!(is_secret(t, c), "{t}.{c}");
        }
        for (t, c) in [
            ("staff", "must_change_password"),
            ("staff", "email"),
            ("student", "id"),
            ("staff_session", "staff_id"),
            ("audit_log", "after"),
        ] {
            assert!(!is_secret(t, c), "{t}.{c}");
        }
    }

    #[test]
    fn sheet_names_fit_excel() {
        let long = "announcement_recipient_delivery_attempts";
        let a = sheet_name(long, &[]);
        assert_eq!(a.chars().count(), 31);
        let b = sheet_name(long, std::slice::from_ref(&a));
        assert_eq!(b.chars().count(), 31);
        assert!(b.ends_with("~2") && b != a);
        assert_eq!(sheet_name("a/b[c]", &[]), "a_b_c_");
        assert_eq!(
            cell_text(&"x".repeat(40_000)).chars().count(),
            MAX_CELL_CHARS
        );
    }
}
