//! Usage and Quotas (#26): metered Pushes per Channel this calendar month
//! against `[quotas.<kind>] metered_per_month` (for LINE, the package chosen on
//! the page once there is one, [`package`]), all read from
//! `usage_daily` (never from a platform API).
//!
//! | What | Where |
//! |---|---|
//! | Usage page, `GET /admin/usage` | `kernel.usage.view`, nav order 85 |
//! | Quota banner in the `#usage-banner` slot of every back-office page | any Staff; links to the page only for holders of `kernel.usage.view` |
//! | `POST /admin/usage/package`, the LINE package (#115, [`package`]) | `kernel.setting.edit` |
//! | Quota emails to every active Administrator | Job `kernel.usage.watch`, every 10 minutes when a Quota is in force |
//!
//! Levels ([`quota::Level`]): `Warn` at 80 % of the cap, `Over` at 100 %.
//! Sending is never blocked. The banner is rendered server-side: the Staff
//! guard reads this month's level once per full-page GET (not for POSTs,
//! HTMX fragments or `.json`/`.csv`; see [`banner`]); a
//! failing read shows no banner rather than breaking the page. The emails
//! lag the banner by at most 10 minutes (one Job run).
//!
//! Usage is not personal data: reading the page writes no audit row.

mod banner;
mod chart;
mod package;
mod package_page;
mod page;
pub(crate) mod quota;
mod record;
mod store;
mod texts;
mod texts_package;
mod watch;

use axum::routing::{get, post};

pub(crate) use banner::banner;
pub(crate) use package::in_force as quota_in_force;
pub(crate) use package::{KIND as LINE_KIND, extra as line_extra};
pub(crate) use quota::Level;
pub(crate) use watch::{administrators, enqueue as enqueue_watch};

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
        .messages(texts_package::DEFAULTS)
        .back_office_page(VIEW, PATH, "kernel.usage.title", 85, get(page::page))
        .back_office(
            package_page::EDIT,
            package_page::PATH,
            post(package_page::submit),
        )
        .job(JobSpec::new(watch::KIND), watch::run);
}

/// One Channel's metered Pushes this calendar month against its Quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MonthSummary {
    /// Metered units so far this local month.
    pub(crate) used: i64,
    /// `[quotas.<kind>] metered_per_month` or the LINE package; `None` when there is no Quota
    /// (none configured, or 0).
    pub(crate) cap: Option<u64>,
    /// How close `used` is to `cap`.
    pub(crate) level: Level,
}

/// This month's usage of each Channel of `kinds` (from `usage_daily`, read
/// once), in that order, for pages that show a cost next to the Quota: the
/// Announcements send page shows "ใช้ไปแล้ว X/Y เดือนนี้"
/// (`kernel.usage.send_used`, or `kernel.usage.send_used_no_cap` without a
/// Quota) beside each push estimate. A kind the Deployment does not have
/// reads as 0 with its configured cap, if any.
pub(crate) async fn month_summaries(
    d: &Deployment,
    kinds: &[String],
) -> Result<Vec<(String, MonthSummary)>> {
    if kinds.is_empty() {
        return Ok(Vec::new());
    }
    let (_, months) = store::this_month_of(d, kinds).await?;
    Ok(months
        .into_iter()
        .map(|m| {
            let (used, cap) = (m.used(), m.cap);
            let level = quota::level(used, cap);
            (m.kind, MonthSummary { used, cap, level })
        })
        .collect())
}
