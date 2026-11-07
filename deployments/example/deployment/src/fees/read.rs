//! Reading a fees file with [`harpseal::sheet`]: every visible sheet with
//! content holds `รหัสนักศึกษา | ชื่อ-สกุล | ยอดค้างชำระ` (title rows above
//! it are fine), the amount in baht with up to two decimals. Totals and
//! notes rows at the bottom are skipped and listed; hidden sheets are left
//! out and listed. Every other doubt is a problem naming the sheet and
//! row, and a file with any problem cannot be imported.

use std::collections::HashSet;

use harpseal::sheet::{self, Blank, SkippedRow, TableError, TableSpec, Workbook};

/// The student id column.
pub const CODE: &str = "รหัสนักศึกษา";
/// The name column (shown, not imported).
pub const NAME: &str = "ชื่อ-สกุล";
/// The amount column, in baht.
pub const AMOUNT: &str = "ยอดค้างชำระ";

/// One balance to import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fee {
    /// The sheet it came from (empty for a CSV).
    pub sheet: String,
    /// Its row number in that sheet (1-based).
    pub row: usize,
    /// The student id.
    pub code: String,
    /// The name as typed.
    pub name: String,
    /// The amount in satang.
    pub satang: i64,
}

/// Why a row or sheet cannot be imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The sheet (empty for a CSV).
    pub sheet: String,
    /// The row (0 when the whole sheet is at fault).
    pub row: usize,
    /// What is wrong, in Thai.
    pub text: String,
}

/// What a file holds.
#[derive(Debug, Default)]
pub struct Parsed {
    /// The balances, in file order.
    pub fees: Vec<Fee>,
    /// What blocks the import.
    pub problems: Vec<Problem>,
    /// Totals and notes rows left out.
    pub skipped: Vec<SkippedRow>,
    /// Hidden sheets left out.
    pub hidden: Vec<String>,
}

impl Parsed {
    /// Whether the file can be imported.
    pub fn importable(&self) -> bool {
        self.problems.is_empty() && !self.fees.is_empty()
    }
}

/// An id is eight digits (`[student] id_pattern`).
fn is_code(s: &str) -> bool {
    s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit())
}

/// Reads every sheet of `book`.
pub fn parse(book: &Workbook) -> Parsed {
    let spec = TableSpec::new([CODE, NAME, AMOUNT])
        .key(CODE)
        .skip_footer([AMOUNT]);
    let mut out = Parsed::default();
    let mut seen = HashSet::new();
    for s in book.sheets() {
        if s.is_hidden() {
            out.hidden.push(s.name().to_string());
            continue;
        }
        if s.is_empty() {
            continue;
        }
        let problem = |row: usize, text: String| Problem {
            sheet: s.name().to_string(),
            row,
            text,
        };
        let table = match s.table(&spec) {
            Ok(t) => t,
            Err(TableError::Header(h)) => {
                let text = if h.missing.is_empty() {
                    format!("หัวตารางซ้ำ: {}", h.repeated.join(", "))
                } else {
                    format!("ไม่พบหัวตาราง (ขาด {})", h.missing.join(", "))
                };
                out.problems.push(problem(h.row, text));
                continue;
            }
            Err(_) => {
                out.problems.push(problem(0, "ไม่มีข้อมูลใต้หัวตาราง".into()));
                continue;
            }
        };
        out.skipped.extend(table.footer().iter().cloned());
        for r in table.rows() {
            let bad: Vec<String> = r
                .unreadable()
                .map(|(col, code, cell)| format!("{col} ({cell}) อ่านไม่ได้: {code}"))
                .collect();
            if !bad.is_empty() {
                out.problems.push(problem(r.number(), bad.join("; ")));
                continue;
            }
            let code = r.get(CODE).unwrap_or_default().to_string();
            if !is_code(&code) {
                out.problems
                    .push(problem(r.number(), format!("รหัสนักศึกษาไม่ถูกต้อง: {code:?}")));
                continue;
            }
            let raw = r.get(AMOUNT).unwrap_or_default();
            let satang = match sheet::number_scaled(raw, 2, Blank::Missing) {
                Ok(Some(v)) if v >= 0 => v,
                Ok(Some(_)) => {
                    out.problems.push(problem(r.number(), "ยอดติดลบ".into()));
                    continue;
                }
                Ok(None) => {
                    out.problems.push(problem(r.number(), "ไม่มียอด".into()));
                    continue;
                }
                Err(_) => {
                    let text = format!("ยอดไม่ใช่ตัวเลข: {raw:?}");
                    out.problems.push(problem(r.number(), text));
                    continue;
                }
            };
            if !seen.insert(code.clone()) {
                let text = format!("รหัส {code} ซ้ำกับแถวก่อนหน้า");
                out.problems.push(problem(r.number(), text));
                continue;
            }
            out.fees.push(Fee {
                sheet: s.name().to_string(),
                row: r.number(),
                code,
                name: r.get(NAME).unwrap_or_default().to_string(),
                satang,
            });
        }
    }
    out
}

/// `1250050` → `12,500.50`.
pub fn baht(satang: i64) -> String {
    let whole = (satang / 100).to_string();
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{grouped}.{:02}", satang % 100)
}

#[cfg(test)]
mod tests {
    use super::{Workbook, baht, parse};

    #[test]
    fn a_csv_under_a_title_row_reads_with_its_totals_skipped() {
        let csv = "ค่าธรรมเนียม 2/2569\n\
                   รหัสนักศึกษา,ชื่อ-สกุล,ยอดค้างชำระ\n\
                   68010001,นกน้อย ทะเลใส,\"12,500\"\n\
                   68010002,ปลาทอง อ่าวคราม,800.5\n\
                   รวม,,\"13,300.50\"\n";
        let p = parse(&Workbook::open(csv.as_bytes(), "fees.csv").unwrap());
        assert!(p.importable(), "{:?}", p.problems);
        assert_eq!(p.fees.len(), 2);
        assert_eq!((p.fees[1].row, p.fees[1].satang), (4, 80_050));
        assert_eq!(p.skipped.len(), 1);
    }

    #[test]
    fn baht_groups_thousands() {
        assert_eq!(baht(1_250_050), "12,500.50");
        assert_eq!(baht(5), "0.05");
        assert_eq!(baht(100_000_000), "1,000,000.00");
    }
}
