//! Dates and numbers as Thai Staff read them (#125): `9 พ.ย. 2569 14:30`
//! with the Buddhist year (Gregorian + 543), in the Deployment's
//! `utc_offset`, and `15,000` with thousands separators. ISO dates stay in
//! the "ข้อมูลทางเทคนิค" disclosure, form values and CSV files.
//!
//! No date crate: the civil date comes from Howard Hinnant's
//! days-from-civil algorithm, as everywhere else in the Core.

use crate::PageState;

/// Thai month abbreviations, January first.
const MONTHS: [&str; 12] = [
    "ม.ค.",
    "ก.พ.",
    "มี.ค.",
    "เม.ย.",
    "พ.ค.",
    "มิ.ย.",
    "ก.ค.",
    "ส.ค.",
    "ก.ย.",
    "ต.ค.",
    "พ.ย.",
    "ธ.ค.",
];

/// The Buddhist year of a Gregorian one.
const BE: i64 = 543;

/// `(year, month, day, minute of the day)` of `ms` (UTC milliseconds) at
/// `offset_minutes` east of UTC.
fn civil(ms: i64, offset_minutes: i32) -> (i64, i64, i64, i64) {
    let local = ms.div_euclid(60_000) + i64::from(offset_minutes);
    let (days, minute) = (local.div_euclid(1_440), local.rem_euclid(1_440));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d, minute)
}

fn month_name(m: i64) -> &'static str {
    usize::try_from(m - 1)
        .ok()
        .and_then(|i| MONTHS.get(i))
        .copied()
        .unwrap_or("")
}

/// `ms` (UTC) as a Thai date at `offset_minutes` east: `9 พ.ย. 2569`.
pub fn thai_date(ms: i64, offset_minutes: i32) -> String {
    let (y, m, d, _) = civil(ms, offset_minutes);
    format!("{d} {} {}", month_name(m), y + BE)
}

/// `ms` (UTC) as a Thai date and time at `offset_minutes` east:
/// `9 พ.ย. 2569 14:30` (24-hour clock).
pub fn thai_datetime(ms: i64, offset_minutes: i32) -> String {
    let (_, _, _, minute) = civil(ms, offset_minutes);
    format!(
        "{} {:02}:{:02}",
        thai_date(ms, offset_minutes),
        minute / 60,
        minute % 60
    )
}

/// A calendar month `YYYY-MM` as `พ.ย. 2569`; anything else unchanged.
pub fn thai_month(iso: &str) -> String {
    match iso.split_once('-').and_then(|(y, m)| {
        let (y, m): (i64, i64) = (y.parse().ok()?, m.parse().ok()?);
        (iso.len() == 7 && (1..=12).contains(&m)).then_some((y, m))
    }) {
        Some((y, m)) => format!("{} {}", month_name(m), y + BE),
        None => iso.to_string(),
    }
}

/// A calendar day `YYYY-MM-DD` as `9 พ.ย. 2569`; anything else unchanged.
pub fn thai_day(iso: &str) -> String {
    let parts = (iso.len() == 10)
        .then(|| iso.get(..7).zip(iso.get(8..)))
        .flatten()
        .and_then(|(month, d)| Some((month, d.parse::<u8>().ok()?)));
    match parts {
        Some((month, d)) if (1..=31).contains(&d) && iso.as_bytes()[7] == b'-' => {
            let shown = thai_month(month);
            if shown == month {
                iso.to_string()
            } else {
                format!("{d} {shown}")
            }
        }
        _ => iso.to_string(),
    }
}

/// `n` with a comma every three digits: `15,000`, `-1,234`. Takes any
/// integer type (`usize`, `u64`, `i64`, …).
pub fn number(n: impl TryInto<i128>) -> String {
    // Every integer type up to 64 bits (and usize) fits in an i128.
    let n: i128 = n.try_into().unwrap_or(i128::MAX);
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// [`thai_datetime`] in the Deployment's `utc_offset`, for a page handler.
pub fn local_datetime(s: &PageState, ms: i64) -> String {
    thai_datetime(ms, s.config.utc_offset_minutes().unwrap_or(0))
}

/// [`thai_date`] in the Deployment's `utc_offset`, for a page handler.
pub fn local_date(s: &PageState, ms: i64) -> String {
    thai_date(ms, s.config.utc_offset_minutes().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-09T07:30:00Z.
    const MS: i64 = 1_794_209_400_000;

    #[test]
    fn dates_use_the_buddhist_year_and_the_offset() {
        assert_eq!(thai_datetime(MS, 420), "9 พ.ย. 2569 14:30");
        assert_eq!(thai_datetime(MS, 0), "9 พ.ย. 2569 07:30");
        assert_eq!(thai_date(MS, -480), "8 พ.ย. 2569");
        assert_eq!(thai_datetime(0, 0), "1 ม.ค. 2513 00:00");
        // 2000-02-29 and the last month.
        assert_eq!(thai_date(951_782_400_000, 0), "29 ก.พ. 2543");
        assert_eq!(thai_date(1_798_675_200_000, 0), "31 ธ.ค. 2569");
    }

    #[test]
    fn midnight_belongs_to_the_local_day() {
        // 2026-11-08T17:00:00Z is midnight on the 9th in Bangkok.
        let midnight = 1_794_157_200_000;
        assert_eq!(thai_datetime(midnight, 420), "9 พ.ย. 2569 00:00");
        assert_eq!(thai_datetime(midnight - 60_000, 420), "8 พ.ย. 2569 23:59");
        // New Year in Bangkok is still the old year in UTC.
        let new_year = 1_798_736_400_000; // 2026-12-31T17:00:00Z
        assert_eq!(thai_datetime(new_year, 420), "1 ม.ค. 2570 00:00");
        assert_eq!(thai_datetime(new_year, 0), "31 ธ.ค. 2569 17:00");
        // A negative offset before 1970 (div_euclid, not truncation).
        assert_eq!(thai_datetime(0, -60), "31 ธ.ค. 2512 23:00");
    }

    #[test]
    fn months_and_days_from_iso() {
        assert_eq!(thai_month("2026-11"), "พ.ย. 2569");
        assert_eq!(thai_month("2026-01"), "ม.ค. 2569");
        assert_eq!(thai_month("2026-13"), "2026-13");
        assert_eq!(thai_month("nope"), "nope");
        assert_eq!(thai_day("2026-11-09"), "9 พ.ย. 2569");
        assert_eq!(thai_day("2026-11-31x"), "2026-11-31x");
        assert_eq!(thai_day("2026-00-09"), "2026-00-09");
        assert_eq!(thai_day("2026-11-00"), "2026-11-00");
    }

    #[test]
    fn numbers_have_thousands_separators() {
        assert_eq!(number(0), "0");
        assert_eq!(number(u64::MAX), "18,446,744,073,709,551,615");
        assert_eq!(number(12_345_usize), "12,345");
        assert_eq!(number(999), "999");
        assert_eq!(number(1_000), "1,000");
        assert_eq!(number(15_000), "15,000");
        assert_eq!(number(1_234_567), "1,234,567");
        assert_eq!(number(-1_234), "-1,234");
        assert_eq!(number(i64::MIN), "-9,223,372,036,854,775,808");
    }
}
