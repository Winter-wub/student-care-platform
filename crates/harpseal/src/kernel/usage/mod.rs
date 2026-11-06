//! Usage and Quotas (#26): metered Pushes per Channel this calendar month
//! against `[quotas.<kind>] metered_per_month`, all read from
//! `usage_daily` (never from a platform API).
//!
//! | What | Where |
//! |---|---|
//! | Usage page, `GET /admin/usage` | `kernel.usage.view`, nav order 85 |
//! | Quota banner in the `#usage-banner` slot of every back-office page | any Staff; links to the page only for holders of `kernel.usage.view` |
//! | Quota emails to every active Administrator | Job `kernel.usage.watch`, every 10 minutes |
//!
//! Levels ([`quota::Level`]): `Warn` at 80 % of the cap, `Over` at 100 %.
//! Sending is never blocked. The banner is rendered server-side: the Staff
//! guard reads this month's level once per request (see [`banner`]); a
//! failing read shows no banner rather than breaking the page. The emails
//! lag the banner by at most 10 minutes (one Job run).
//!
//! Usage is not personal data: reading the page writes no audit row.

mod banner;
mod chart;
mod page;
pub(crate) mod quota;
mod store;
mod texts;
mod watch;

use axum::routing::get;

pub(crate) use banner::banner;
pub(crate) use quota::Level;

use crate::manifest::JobSpec;
use crate::module::ModuleRegistrar;
use crate::{Deployment, Result};

/// Permission of the usage page.
pub(crate) const VIEW: &str = "kernel.usage.view";
/// The usage page, under `/admin`.
pub(crate) const PATH: &str = "/usage";
/// The page's full path, linked from the banner and the emails.
pub(crate) const FULL_PATH: &str = "/admin/usage";

/// Registers the page, its texts and the watch Job on the Kernel.
pub(crate) fn register(r: &mut ModuleRegistrar) {
    r.messages(texts::DEFAULTS)
        .back_office_page(VIEW, PATH, "kernel.usage.title", 85, get(page::page))
        .job(JobSpec::scheduled(watch::KIND, watch::SCHEDULE), watch::run);
}

/// One Channel's metered Pushes this calendar month against its Quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MonthSummary {
    /// Metered units so far this local month.
    pub(crate) used: i64,
    /// `[quotas.<kind>] metered_per_month`; `None` when there is no Quota
    /// (none configured, or 0).
    pub(crate) cap: Option<u64>,
    /// How close `used` is to `cap`.
    pub(crate) level: Level,
}

/// This month's usage of Channel `kind` (from `usage_daily`), for pages
/// that show a cost next to the Quota: the Announcements send page shows
/// "ใช้ไปแล้ว X/Y เดือนนี้" (`kernel.usage.send_used`, or
/// `kernel.usage.send_used_no_cap` without a Quota) beside its push
/// estimate. A kind the Deployment does not have reads as 0 with its
/// configured cap, if any.
pub(crate) async fn month_summary(d: &Deployment, kind: &str) -> Result<MonthSummary> {
    let offset = d.config().utc_offset_minutes().unwrap_or(0);
    let month = quota::month_at(d.services().now_ms(), offset);
    let rows = store::rows_since(d.services(), &format!("{month}-01")).await?;
    let m = quota::month(&rows, &[kind.to_string()], &d.config().quotas, &month);
    let used = m.first().map_or(0, |m| m.used());
    let cap = quota::cap_of(&d.config().quotas, kind);
    Ok(MonthSummary {
        used,
        cap,
        level: quota::level(used, cap),
    })
}
