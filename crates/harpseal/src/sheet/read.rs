//! Reading the bytes into [`Sheet`]s of trimmed, non-empty rows.

use std::collections::BTreeMap;
use std::io::Cursor;

use calamine::{Data, Reader, SheetType, SheetVisible, Xlsx};

use super::{Cell, Limits, RawRow, Sheet, SheetError, column_name, csv};

/// The one sheet of a CSV.
pub(super) fn csv(bytes: &[u8], limits: Limits) -> Result<Sheet, SheetError> {
    let text = std::str::from_utf8(bytes).map_err(|_| SheetError::NotUtf8)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let records = csv::records(text, csv::delimiter(text)).map_err(|e| match e {
        csv::CsvError::Unterminated(row) | csv::CsvError::AfterQuote(row) => {
            SheetError::Csv { row }
        }
    })?;
    let mut rows = Vec::new();
    for (i, record) in records.into_iter().enumerate() {
        let cells: Vec<Option<Cell>> = record.iter().map(|c| text_cell(c)).collect();
        if cells.iter().all(Option::is_none) {
            continue;
        }
        if rows.len() >= limits.max_rows + Limits::SLACK {
            return Err(SheetError::TooManyRows);
        }
        rows.push(RawRow {
            number: i + 1,
            cells,
        });
    }
    Ok(Sheet {
        name: String::new(),
        rows,
        hidden: false,
    })
}

/// Every worksheet, read cell by cell (never as calamine's dense range).
pub(super) fn xlsx(bytes: &[u8], limits: Limits) -> Result<Vec<Sheet>, SheetError> {
    let mut book: Xlsx<_> =
        calamine::open_workbook_from_rs(Cursor::new(bytes)).map_err(|_| SheetError::Xlsx)?;
    let names: Vec<(String, bool)> = book
        .sheets_metadata()
        .iter()
        .filter(|s| s.typ == SheetType::WorkSheet)
        .map(|s| (s.name.clone(), s.visible != SheetVisible::Visible))
        .collect();
    if names.is_empty() {
        return Err(SheetError::Xlsx);
    }
    // Before reading any: 5 000 small sheets would pass the row checks.
    if names.len() > limits.max_sheets {
        return Err(SheetError::TooManySheets);
    }
    let mut total = 0usize;
    let mut sheets = Vec::with_capacity(names.len());
    for (n, (name, hidden)) in names.into_iter().enumerate() {
        let rows = xlsx_rows(&mut book, &name, limits)?;
        total += rows.len();
        // SLACK per sheet, so at most SLACK × max_sheets in all.
        if total > limits.max_rows + Limits::SLACK * (n + 1) {
            return Err(SheetError::TooManyRows);
        }
        sheets.push(Sheet { name, rows, hidden });
    }
    Ok(sheets)
}

fn xlsx_rows(
    book: &mut Xlsx<Cursor<&[u8]>>,
    name: &str,
    limits: Limits,
) -> Result<Vec<RawRow>, SheetError> {
    let mut cells = book
        .worksheet_cells_reader(name)
        .map_err(|_| SheetError::Xlsx)?;
    let mut rows: BTreeMap<u32, Vec<Option<Cell>>> = BTreeMap::new();
    while let Some(cell) = cells.next_cell().map_err(|_| SheetError::Xlsx)? {
        let (r, c) = cell.get_position();
        let (r, c) = (r as usize, c as usize);
        if r >= limits.max_rows + Limits::SLACK {
            return Err(SheetError::TooManyRows);
        }
        if c >= limits.max_columns {
            return Err(SheetError::TooManyColumns);
        }
        let value = match Data::from(cell.get_value().clone()) {
            Data::Error(e) => Some(Cell::Unreadable {
                code: e.to_string(),
                cell: format!("{}{}", column_name(c), r + 1),
            }),
            // A whole number an f64 cannot hold exactly (an id or a
            // payment reference of 16+ digits): never print a rounded one.
            Data::Float(f) if f.fract() == 0.0 && f.abs() >= MAX_EXACT => Some(Cell::Unreadable {
                code: TOO_LONG.into(),
                cell: format!("{}{}", column_name(c), r + 1),
            }),
            other => text_cell(&cell_text(&other)),
        };
        let Some(value) = value else { continue };
        let row = rows.entry(r as u32).or_default();
        if row.len() <= c {
            row.resize(c + 1, None);
        }
        row[c] = Some(value);
    }
    Ok(rows
        .into_iter()
        .map(|(r, cells)| RawRow {
            number: r as usize + 1,
            cells,
        })
        .collect())
}

/// A trimmed text cell; `None` when empty.
fn text_cell(raw: &str) -> Option<Cell> {
    let t = raw.trim();
    (!t.is_empty()).then(|| Cell::Text(t.to_string()))
}

/// A cell as text: integers without `.0`, dates as `YYYY-MM-DD` (with
/// ` HH:MM:SS` when the cell has a time of day), an error as its code.
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

/// 2^53: every whole number below it is exact in an `f64`. 2^53 itself is
/// not safe: a file holding `9007199254740993` reads as 2^53 too.
pub(crate) const MAX_EXACT: f64 = 9_007_199_254_740_992.0;

/// The code of a [`Cell::Unreadable`] holding a whole number of 2^53
/// (9 007 199 254 740 992) or more, which cannot be read exactly (type it
/// as text instead).
pub const TOO_LONG: &str = "#TOO_LONG";

/// A numeric cell as text. A whole number is exact (below 2^53; larger
/// ones are [`TOO_LONG`] before they get here), so a 16-digit id stays
/// itself. A number with a fractional part is rounded to 15 significant
/// digits first (what Excel shows): a calculated `12500.000000000002`
/// reads `12500`, an `=AVERAGE` GPA of `3.3333333333333335` reads
/// `3.33333333333333`, so the number helpers accept them. Text cells are
/// never rounded.
pub(crate) fn number_text(f: f64) -> String {
    let f = if f.is_finite() && f.fract() != 0.0 {
        format!("{f:.14e}").parse::<f64>().unwrap_or(f)
    } else {
        f
    };
    if f.fract() == 0.0 && f.abs() <= MAX_EXACT {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::number_text;
    use crate::sheet::{Blank, number, number_scaled};

    #[test]
    fn calculated_cells_read_as_excel_shows_them() {
        assert_eq!(number_text(12500.000000000002), "12500");
        assert_eq!(number_text(3.3333333333333335), "3.33333333333333");
        assert_eq!(number_text(0.30000000000000004), "0.3");
        assert_eq!(number_text(68010001.0), "68010001");
        assert_eq!(number_text(2.5), "2.5");
        assert_eq!(number_text(-0.1), "-0.1");
        assert_eq!(number_text(0.0), "0");
        assert_eq!(number_text(1e-7), "0.0000001");
        // Whole numbers are never rounded: a 16-digit id stays itself.
        assert_eq!(number_text(1234567890123456.0), "1234567890123456");
        assert_eq!(number_text(9007199254740991.0), "9007199254740991");
        assert_eq!(number_text(-9007199254740991.0), "-9007199254740991");
        assert_eq!(number_text(999999999999999.0), "999999999999999");
        let amount = number_text(12500.000000000002);
        assert_eq!(
            number_scaled(&amount, 2, Blank::Missing),
            Ok(Some(1_250_000))
        );
        let gpa = number_text(3.3333333333333335);
        assert_eq!(number(&gpa, Blank::Missing), Ok(Some(3.33333333333333)));
    }
}
