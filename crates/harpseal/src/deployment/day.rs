//! Civil days for `usage_daily`.

/// `YYYY-MM-DD` of the instant `ms` (UTC milliseconds) in a zone
/// `offset_minutes` east of UTC (Howard Hinnant's civil-from-days).
pub(crate) fn civil_day(ms: i64, offset_minutes: i32) -> String {
    let local = ms + i64::from(offset_minutes) * 60_000;
    let z = local.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::civil_day;

    #[test]
    fn days_follow_the_offset() {
        // 2026-11-05T00:00:00Z
        let ms = 1_793_836_800_000;
        assert_eq!(civil_day(ms, 0), "2026-11-05");
        assert_eq!(civil_day(ms - 1, 0), "2026-11-04");
        // 2026-11-04T17:00Z is already the 5th in Bangkok.
        assert_eq!(civil_day(ms - 7 * 3_600_000, 420), "2026-11-05");
        assert_eq!(civil_day(ms - 7 * 3_600_000 - 1, 420), "2026-11-04");
        assert_eq!(civil_day(0, 0), "1970-01-01");
        assert_eq!(civil_day(951_782_400_000, 0), "2000-02-29");
    }
}
