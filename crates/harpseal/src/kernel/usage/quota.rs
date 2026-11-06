//! Quota evaluation, pure over `usage_daily` rows (#26).
//!
//! `usage_daily.day` is already the Deployment's local day (`YYYY-MM-DD`
//! in `utc_offset`, written by the send path), so a calendar month is the
//! rows whose `day` starts with `YYYY-MM`. The cap is
//! `[quotas.<kind>] metered_per_month`; a Channel without one, or with a
//! cap of 0 (no allowance to measure against), has no Quota: its usage is
//! shown and it never warns.

use std::collections::BTreeMap;

use crate::config::QuotaConfig;
use crate::kernel::students::view::format_ms;

/// How close a Channel is to its Quota this month. Ordered: `Ok < Warn <
/// Over`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Level {
    /// Below 80 % (or no Quota).
    Ok,
    /// 80 % or more.
    Warn,
    /// 100 % or more.
    Over,
}

impl Level {
    /// `ok`, `warn` or `over` (CSS classes, the notification record).
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Warn => "warn",
            Level::Over => "over",
        }
    }
}

/// One `usage_daily` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) channel: String,
    /// Local `YYYY-MM-DD`.
    pub(crate) day: String,
    pub(crate) metered: i64,
    pub(crate) free: i64,
    pub(crate) failed: i64,
}

/// Sums of one Channel over a period.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Totals {
    pub(crate) metered: i64,
    pub(crate) free: i64,
    pub(crate) failed: i64,
}

impl Totals {
    fn add(&mut self, r: &Row) {
        self.metered += r.metered;
        self.free += r.free;
        self.failed += r.failed;
    }
}

/// One Channel's calendar month against its Quota.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChannelMonth {
    pub(crate) kind: String,
    /// `None`: no Quota (none configured, or 0).
    pub(crate) cap: Option<u64>,
    pub(crate) totals: Totals,
    pub(crate) level: Level,
}

impl ChannelMonth {
    /// Metered units this month.
    pub(crate) fn used(&self) -> i64 {
        self.totals.metered
    }

    /// Whole percent of the cap used (rounded down), when there is a cap.
    pub(crate) fn percent(&self) -> Option<i64> {
        let cap = i64::try_from(self.cap?).unwrap_or(i64::MAX);
        Some(self.used().max(0).saturating_mul(100) / cap)
    }
}

/// The cap of `kind`: `None` without `[quotas.<kind>]` or with 0.
pub(crate) fn cap_of(quotas: &BTreeMap<String, QuotaConfig>, kind: &str) -> Option<u64> {
    quotas
        .get(kind)
        .map(|q| q.metered_per_month)
        .filter(|c| *c > 0)
}

/// `Warn` at 80 % of `cap`, `Over` at 100 %, in integers (240 of 300 is
/// `Warn`, 239 is not).
pub(crate) fn level(used: i64, cap: Option<u64>) -> Level {
    let Some(cap) = cap else {
        return Level::Ok;
    };
    let used = i128::from(used.max(0));
    let cap = i128::from(cap);
    if used >= cap {
        Level::Over
    } else if used * 100 >= cap * 80 {
        Level::Warn
    } else {
        Level::Ok
    }
}

/// The local day `YYYY-MM-DD` of the instant `ms` (the send path's
/// `usage_daily.day`).
pub(crate) fn civil_day(ms: i64, offset_minutes: i32) -> String {
    format_ms(ms, offset_minutes)[..10].to_string()
}

/// The local month `YYYY-MM` of the instant `now_ms`.
pub(crate) fn month_at(now_ms: i64, offset_minutes: i32) -> String {
    civil_day(now_ms, offset_minutes)[..7].to_string()
}

/// Each Channel of `kinds` (in that order) in `month` (`YYYY-MM`).
pub(crate) fn month(
    rows: &[Row],
    kinds: &[String],
    quotas: &BTreeMap<String, QuotaConfig>,
    month: &str,
) -> Vec<ChannelMonth> {
    kinds
        .iter()
        .map(|kind| {
            let mut totals = Totals::default();
            rows.iter()
                .filter(|r| r.channel == *kind && r.day.get(..7) == Some(month))
                .for_each(|r| totals.add(r));
            let cap = cap_of(quotas, kind);
            ChannelMonth {
                kind: kind.clone(),
                cap,
                totals,
                level: level(totals.metered, cap),
            }
        })
        .collect()
}

/// The highest level over all Channels (`Ok` for none).
pub(crate) fn highest(months: &[ChannelMonth]) -> Level {
    months.iter().map(|m| m.level).max().unwrap_or(Level::Ok)
}

/// The local days `today` and the `n - 1` before it, oldest first.
pub(crate) fn last_days(now_ms: i64, offset_minutes: i32, n: i64) -> Vec<String> {
    (0..n)
        .rev()
        .map(|i| civil_day(now_ms - i * 86_400_000, offset_minutes))
        .collect()
}

/// `kind`'s totals on each of `days`, zeros where there is no row.
pub(crate) fn per_day(rows: &[Row], kind: &str, days: &[String]) -> Vec<Totals> {
    days.iter()
        .map(|d| {
            let mut t = Totals::default();
            rows.iter()
                .filter(|r| r.channel == kind && r.day == *d)
                .for_each(|r| t.add(r));
            t
        })
        .collect()
}

/// `this` (`YYYY-MM`) and the `n - 1` months before it, newest first.
pub(crate) fn last_months(this: &str, n: u32) -> Vec<String> {
    let y: i64 = this.get(..4).and_then(|y| y.parse().ok()).unwrap_or(1970);
    let m: i64 = this.get(5..7).and_then(|m| m.parse().ok()).unwrap_or(1);
    let index = y * 12 + (m - 1);
    (0..i64::from(n))
        .map(|i| {
            let k = index - i;
            format!("{:04}-{:02}", k.div_euclid(12), k.rem_euclid(12) + 1)
        })
        .collect()
}

/// `kind`'s totals in `month` (`YYYY-MM`).
pub(crate) fn in_month(rows: &[Row], kind: &str, month: &str) -> Totals {
    let mut t = Totals::default();
    rows.iter()
        .filter(|r| r.channel == kind && r.day.get(..7) == Some(month))
        .for_each(|r| t.add(r));
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    const BKK: i32 = 420;
    /// 2026-11-30T16:30:00Z = 23:30 on 30 November in +07:00.
    const NOV_30_2330: i64 = 1_796_056_200_000;

    fn row(channel: &str, day: &str, metered: i64) -> Row {
        Row {
            channel: channel.into(),
            day: day.into(),
            metered,
            free: 1,
            failed: 2,
        }
    }

    fn quotas(cap: u64) -> BTreeMap<String, QuotaConfig> {
        let toml = format!(
            "name = \"X\"\ndefault_locale = \"th\"\n[quotas.line]\nmetered_per_month = {cap}"
        );
        crate::DeploymentConfig::from_toml(&toml).unwrap().quotas
    }

    #[test]
    fn levels_at_80_and_100_percent() {
        assert_eq!(level(239, Some(300)), Level::Ok);
        assert_eq!(level(240, Some(300)), Level::Warn);
        assert_eq!(level(299, Some(300)), Level::Warn);
        assert_eq!(level(300, Some(300)), Level::Over);
        assert_eq!(level(10_000, Some(300)), Level::Over);
        assert_eq!(level(10_000, None), Level::Ok);
        assert!(Level::Ok < Level::Warn && Level::Warn < Level::Over);
    }

    #[test]
    fn a_cap_of_zero_is_no_quota() {
        let q = quotas(0);
        assert_eq!(cap_of(&q, "line"), None);
        let m = month(&[row("line", "2026-11-01", 5)], &["line".into()], &q, "2026-11");
        assert_eq!((m[0].cap, m[0].level, m[0].used()), (None, Level::Ok, 5));
        assert_eq!(m[0].percent(), None);
    }

    #[test]
    fn the_month_follows_the_local_day() {
        assert_eq!(month_at(NOV_30_2330, BKK), "2026-11");
        assert_eq!(month_at(NOV_30_2330, 0), "2026-11");
        // One hour later: 00:30 on 1 December in Bangkok, still November in UTC.
        assert_eq!(month_at(NOV_30_2330 + 3_600_000, BKK), "2026-12");
        assert_eq!(month_at(NOV_30_2330 + 3_600_000, 0), "2026-11");
    }

    #[test]
    fn sums_only_the_month_and_the_channel() {
        let rows = [
            row("line", "2026-10-31", 100),
            row("line", "2026-11-01", 200),
            row("line", "2026-11-30", 40),
            row("telegram", "2026-11-15", 999),
            row("line", "2026-12-01", 7),
        ];
        let kinds = ["line".to_string(), "telegram".to_string()];
        let m = month(&rows, &kinds, &quotas(300), "2026-11");
        assert_eq!(m[0].totals, Totals { metered: 240, free: 2, failed: 4 });
        assert_eq!((m[0].level, m[0].percent()), (Level::Warn, Some(80)));
        assert_eq!((m[1].cap, m[1].level, m[1].used()), (None, Level::Ok, 999));
        assert_eq!(highest(&m), Level::Warn);
        let dec = month(&rows, &kinds, &quotas(300), "2026-12");
        assert_eq!((dec[0].used(), dec[0].level), (7, Level::Ok));
    }

    #[test]
    fn empty_tables() {
        let m = month(&[], &["line".into()], &quotas(300), "2026-11");
        assert_eq!((m[0].used(), m[0].level, m[0].percent()), (0, Level::Ok, Some(0)));
        assert_eq!(highest(&[]), Level::Ok);
        let days = last_days(NOV_30_2330, BKK, 30);
        assert!(per_day(&[], "line", &days).iter().all(|t| *t == Totals::default()));
    }

    #[test]
    fn thirty_local_days_and_six_months() {
        let days = last_days(NOV_30_2330 + 3_600_000, BKK, 30);
        assert_eq!(days.len(), 30);
        assert_eq!(days[29], "2026-12-01");
        assert_eq!(days[0], "2026-11-02");
        assert_eq!(
            last_months("2026-02", 6),
            ["2026-02", "2026-01", "2025-12", "2025-11", "2025-10", "2025-09"]
        );
        let rows = [row("line", "2026-11-02", 3), row("line", "2026-12-01", 4)];
        let d = per_day(&rows, "line", &days);
        assert_eq!((d[0].metered, d[29].metered, d[1].metered), (3, 4, 0));
        assert_eq!(in_month(&rows, "line", "2026-12").metered, 4);
    }
}
