//! The cron subset [`crate::JobSpec::schedule`] accepts, evaluated in the
//! Deployment's `utc_offset`. Hand-written: no cron crate.
//!
//! Five fields separated by spaces: minute (0-59), hour (0-23), day of
//! month (1-31), month (1-12), day of week (0-7, 0 and 7 = Sunday). Each field
//! is a comma-separated list of items, and an item is one of:
//! - `*`: every value;
//! - `*/n`: every `n`th value from the field's first (`*/15` = 0,15,30,45);
//! - `a`: one value;
//! - `a-b`: every value from `a` to `b`, inclusive, `a <= b`.
//!
//! Anything else (names such as `MON`, `a-b/n`, `?`, `L`, `#`, `@daily`,
//! six fields, a value out of range) is refused at build time with
//! [`crate::BuildError::InvalidSchedule`]. As in Vixie cron, when both
//! day of month and day of week are restricted (neither field starts with
//! `*`, so `*/2` counts as unrestricted), a day matching either one is due;
//! otherwise both must match.

/// A parsed schedule: one bit per allowed value of each field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Schedule {
    minute: u64,
    hour: u64,
    dom: u64,
    month: u64,
    dow: u64,
    dom_any: bool,
    dow_any: bool,
}

/// (name, lowest, highest) per field, in order.
const FIELDS: [(&str, u32, u32); 5] = [
    ("minute", 0, 59),
    ("hour", 0, 23),
    ("day of month", 1, 31),
    ("month", 1, 12),
    ("day of week", 0, 7),
];

/// Parses `expr`, or says why it is refused.
pub(crate) fn parse(expr: &str) -> Result<Schedule, String> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(format!("expected 5 fields, found {}", parts.len()));
    }
    let mut masks = [0u64; 5];
    for (i, (part, (name, lo, hi))) in parts.iter().zip(FIELDS).enumerate() {
        masks[i] = field(part, lo, hi).map_err(|e| format!("{name} `{part}`: {e}"))?;
    }
    // 7 is Sunday too.
    if masks[4] & (1 << 7) != 0 {
        masks[4] = (masks[4] & !(1 << 7)) | 1;
    }
    Ok(Schedule {
        minute: masks[0],
        hour: masks[1],
        dom: masks[2],
        month: masks[3],
        dow: masks[4],
        dom_any: parts[2].starts_with('*'),
        dow_any: parts[4].starts_with('*'),
    })
}

fn number(s: &str, lo: u32, hi: u32) -> Result<u32, String> {
    if s.is_empty() || s.len() > 2 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("`{s}` is not a number"));
    }
    let n: u32 = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
    if n < lo || n > hi {
        return Err(format!("{n} is outside {lo}-{hi}"));
    }
    Ok(n)
}

fn range_mask(from: u32, to: u32, step: u32) -> u64 {
    (from..=to)
        .step_by(step as usize)
        .fold(0, |m, v| m | (1u64 << v))
}

fn field(s: &str, lo: u32, hi: u32) -> Result<u64, String> {
    let mut mask = 0;
    for item in s.split(',') {
        mask |= if item == "*" {
            range_mask(lo, hi, 1)
        } else if let Some(step) = item.strip_prefix("*/") {
            let n = number(step, 1, hi.max(1))?;
            range_mask(lo, hi, n)
        } else if let Some((a, b)) = item.split_once('-') {
            let (a, b) = (number(a, lo, hi)?, number(b, lo, hi)?);
            if a > b {
                return Err(format!("range {a}-{b} runs backwards"));
            }
            range_mask(a, b, 1)
        } else {
            1u64 << number(item, lo, hi)?
        };
    }
    Ok(mask)
}

/// Local calendar fields of one minute.
struct Local {
    minute: u32,
    hour: u32,
    day: u32,
    month: u32,
    weekday: u32,
}

/// The local time of `ms` (UTC milliseconds) `offset_minutes` east of UTC
/// (Howard Hinnant's civil-from-days).
fn local(ms: i64, offset_minutes: i32) -> Local {
    let local = ms + i64::from(offset_minutes) * 60_000;
    let days = local.div_euclid(86_400_000);
    let in_day = local.rem_euclid(86_400_000) / 60_000;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    // 1970-01-01 was a Thursday (4).
    let weekday = (days + 4).rem_euclid(7);
    let u = |v: i64| u32::try_from(v).unwrap_or(0);
    Local {
        minute: u(in_day % 60),
        hour: u(in_day / 60),
        day: u(day),
        month: u(month),
        weekday: u(weekday),
    }
}

impl Schedule {
    /// Whether the minute holding `ms` is due, in a zone `offset_minutes`
    /// east of UTC.
    pub(crate) fn is_due(&self, ms: i64, offset_minutes: i32) -> bool {
        let t = local(ms, offset_minutes);
        let has = |mask: u64, v: u32| mask & (1u64 << v) != 0;
        let dom = has(self.dom, t.day);
        let dow = has(self.dow, t.weekday);
        let day = match (self.dom_any, self.dow_any) {
            (false, false) => dom || dow,
            _ => dom && dow,
        };
        has(self.minute, t.minute) && has(self.hour, t.hour) && has(self.month, t.month) && day
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-05T00:00:00Z, a Thursday; 07:00 in Bangkok.
    const T0: i64 = 1_793_836_800_000;
    const MIN: i64 = 60_000;

    #[test]
    fn accepts_the_subset() {
        for ok in [
            "* * * * *",
            "*/15 * * * *",
            "0 7 * * 1-5",
            "0,30 8-17 1,15 */2 0",
            "59 23 31 12 6",
            "  5   4 * * *  ",
        ] {
            parse(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
        }
    }

    #[test]
    fn refuses_everything_else() {
        for bad in [
            "",
            "* * * *",
            "* * * * * *",
            "@daily",
            "60 * * * *",
            "* 24 * * *",
            "* * 0 * *",
            "* * * 13 *",
            "* * * * 8",
            "* * * * MON",
            "*/0 * * * *",
            "1-5/2 * * * *",
            "5-1 * * * *",
            "? * * * *",
            "1,,2 * * * *",
            "-1 * * * *",
            "+1 * * * *",
            "001 * * * *",
            "* * L * *",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn evaluates_in_the_deployment_offset() {
        let s = parse("0 7 * * *").unwrap();
        assert!(s.is_due(T0, 420), "07:00 in Bangkok");
        assert!(s.is_due(T0 + 59_999, 420), "any instant in the minute");
        assert!(!s.is_due(T0 + MIN, 420));
        assert!(!s.is_due(T0, 0), "00:00 UTC is not 07:00");
        assert!(s.is_due(T0 + 7 * 60 * MIN, 0));
    }

    #[test]
    fn steps_lists_and_weekdays() {
        let s = parse("*/20 * * * 4").unwrap();
        assert!(s.is_due(T0, 0) && s.is_due(T0 + 40 * MIN, 0));
        assert!(!s.is_due(T0 + 10 * MIN, 0));
        assert!(!s.is_due(T0 + 24 * 60 * MIN, 0), "Friday");
        // Restricted day of month OR day of week: the 1st, or a Thursday.
        let either = parse("0 0 1 * 4").unwrap();
        assert!(either.is_due(T0, 0), "a Thursday, the 5th");
        assert!(!either.is_due(T0 + 24 * 60 * MIN, 0), "Friday the 6th");
        let first_of_december = 1_796_083_200_000; // 2026-12-01T00:00Z, Tuesday
        assert!(either.is_due(first_of_december, 0));
        let leap = 951_782_400_000; // 2000-02-29T00:00Z
        assert!(parse("0 0 29 2 *").unwrap().is_due(leap, 0));
    }

    #[test]
    fn seven_is_sunday() {
        let sunday = T0 + 3 * 24 * 60 * MIN; // 2026-11-08
        for expr in ["0 0 * * 7", "0 0 * * 0", "0 0 * * 6-7", "0 0 * * 5,7"] {
            let s = parse(expr).unwrap();
            assert!(s.is_due(sunday, 0), "{expr}");
            assert!(!s.is_due(T0, 0), "{expr} on a Thursday");
        }
        assert_eq!(parse("0 0 * * 7").unwrap(), parse("0 0 * * 0").unwrap());
    }

    #[test]
    fn a_stepped_day_field_counts_as_unrestricted() {
        // `*/2` day of month (odd days) AND Thursday: the 5th is both.
        let s = parse("0 0 */2 * 4").unwrap();
        assert!(s.is_due(T0, 0), "Thursday the 5th");
        assert!(
            !s.is_due(T0 + 24 * 60 * MIN, 0),
            "Friday the 6th: not Thursday"
        );
        assert!(
            !s.is_due(T0 + 7 * 24 * 60 * MIN, 0),
            "Thursday the 12th: even day"
        );
        // Both literal restrictions: either one.
        let either = parse("0 0 1-3 * 4").unwrap();
        assert!(
            either.is_due(T0 + 7 * 24 * 60 * MIN, 0),
            "Thursday the 12th"
        );
    }
}
