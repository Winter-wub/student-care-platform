//! Checking a footer's total against the data (#70 review): a totals row
//! that does not add up is the sign of a data row read as a footer.

use super::number::{Blank, number, number_scaled};
use super::table::Table;

/// Why [`Table::check_footer_total`] fails.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TotalError {
    /// A data or footer row's value under the column is not a number at
    /// the scale (or is unreadable).
    NotANumber {
        /// The sheet's name.
        sheet: String,
        /// The row.
        row: usize,
    },
    /// A footer row's total differs from the sum of the data rows.
    Mismatch {
        /// The sheet's name.
        sheet: String,
        /// The footer row.
        row: usize,
        /// Its total, in `10^-scale` units.
        footer: i64,
        /// The data rows' sum, in the same units.
        data: i64,
    },
}

impl std::fmt::Display for TotalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotANumber { sheet, row } => write!(f, "not a number: `{sheet}` row {row}"),
            Self::Mismatch { sheet, row, .. } => {
                write!(
                    f,
                    "the total on `{sheet}` row {row} is not the sum of the rows"
                )
            }
        }
    }
}

impl std::error::Error for TotalError {}

impl Table {
    /// Checks that every footer row holding a value under `column` equals
    /// the sum of the data rows under it, read exactly with
    /// [`number_scaled`] at `scale` (blank and `-` are zero). For money
    /// imports: `Pla Thong,,12,500` read as a footer above `รวม,,13,500`
    /// with data summing to 1,000 fails here. Footer rows without a value
    /// under `column` are not checked; no footer passes.
    pub fn check_footer_total(&self, column: &str, scale: u32) -> Result<(), TotalError> {
        let value = |r: &super::Row| {
            number_scaled(r.get(column).unwrap_or_default(), scale, Blank::Zero)
                .ok()
                .flatten()
                .filter(|_| {
                    r.cell(column)
                        .is_none_or(|c| matches!(c, super::Cell::Text(_)))
                })
                .ok_or_else(|| TotalError::NotANumber {
                    sheet: r.sheet().to_string(),
                    row: r.number(),
                })
        };
        let mut sum: i64 = 0;
        for r in &self.rows {
            sum = sum
                .checked_add(value(r)?)
                .ok_or_else(|| TotalError::NotANumber {
                    sheet: r.sheet().to_string(),
                    row: r.number(),
                })?;
        }
        for r in self.footer_rows.iter().filter(|r| r.cell(column).is_some()) {
            let footer = value(r)?;
            if footer != sum {
                return Err(TotalError::Mismatch {
                    sheet: r.sheet().to_string(),
                    row: r.number(),
                    footer,
                    data: sum,
                });
            }
        }
        Ok(())
    }
}

/// Whether any footer row holds a non-zero number (or anything that is
/// not a number) under a numeric column: then an all-footer sheet is more
/// likely data than an empty one.
pub(super) fn footer_has_values(t: &Table, numeric: &[String]) -> bool {
    t.footer_rows.iter().any(|r| {
        numeric.iter().any(|c| {
            r.cell(c).is_some()
                && number(r.get(c).unwrap_or_default(), Blank::Zero) != Ok(Some(0.0))
        })
    })
}

#[cfg(test)]
#[path = "total_tests.rs"]
mod tests;
