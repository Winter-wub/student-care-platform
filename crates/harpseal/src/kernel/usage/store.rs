//! Reads `usage_daily` (the only source of every Staff-visible usage
//! number; no platform API is ever called) and the Deployment's Channels.

use super::package::{self, Current};
use super::quota::{self, ChannelMonth, Row};
use crate::config::DeploymentConfig;
use crate::port_types::{Statement, Value};
use crate::{Deployment, Result, Services};

/// `usage_daily` rows whose local day is `since` (`YYYY-MM-DD`) or later,
/// and the LINE package choice, in ONE query (one D1 call per page view,
/// #115): the `setting` row rides along as one more row with `is_pkg` set.
pub(crate) async fn usage_and_package(
    services: &Services,
    config: &DeploymentConfig,
    since: &str,
) -> Result<(Vec<Row>, Current)> {
    // One query, not two: the package row is appended to the usage rows
    // (D1 bills a call per query, and the banner runs on every page view).
    let rows = services
        .db
        .query(
            Statement::new(
                "SELECT channel, day, metered, free, failed, NULL AS pkg, NULL AS is_pkg \
                 FROM usage_daily WHERE day >= ? \
                 UNION ALL \
                 SELECT '', '', 0, 0, 0, value, 1 FROM setting WHERE key = ? AND locale = ''",
            )
            .bind(since)
            .bind(package::KEY),
        )
        .await?;
    let text = |i: usize, c: &str| {
        rows.get(i, c)
            .and_then(Value::as_text)
            .unwrap_or_default()
            .to_string()
    };
    let int = |i: usize, c: &str| rows.get(i, c).and_then(Value::as_i64).unwrap_or(0);
    let mut usage = Vec::new();
    let mut choice = None;
    for i in 0..rows.len() {
        match rows.get(i, "is_pkg").and_then(Value::as_i64) {
            Some(_) => choice = Some(text(i, "pkg")),
            None => usage.push(Row {
                channel: text(i, "channel"),
                day: text(i, "day"),
                metered: int(i, "metered"),
                free: int(i, "free"),
                failed: int(i, "failed"),
            }),
        }
    }
    usage.sort_by(|a, b| (&a.day, &a.channel).cmp(&(&b.day, &b.channel)));
    Ok((usage, package::current_of(config, choice)))
}

/// The Deployment's Channel kinds, in registration order.
pub(crate) fn kinds(d: &Deployment) -> Vec<String> {
    d.channel_kinds()
        .iter()
        .map(|k| k.as_str().to_string())
        .collect()
}

/// The kinds the back office shows Staff (#131): a dev-only Channel only
/// in dev mode. The watcher keeps reading every kind ([`kinds`]).
pub(crate) fn staff_kinds(d: &Deployment) -> Vec<String> {
    d.staff_channel_kinds()
        .iter()
        .map(|k| k.as_str().to_string())
        .collect()
}

/// The current local month (`YYYY-MM`) and each Channel in it.
pub(crate) async fn this_month(d: &Deployment) -> Result<(String, Vec<ChannelMonth>)> {
    this_month_of(d, &kinds(d)).await
}

/// The current local month (`YYYY-MM`) and each of `kinds` in it, from one
/// read (the usage rows and the LINE package together).
pub(crate) async fn this_month_of(
    d: &Deployment,
    kinds: &[String],
) -> Result<(String, Vec<ChannelMonth>)> {
    let offset = d.config().utc_offset_minutes().unwrap_or(0);
    let month = quota::month_at(d.services().now_ms(), offset);
    let (rows, cur) = usage_and_package(d.services(), d.config(), &format!("{month}-01")).await?;
    let caps = package::caps_with(d.config(), &cur);
    let months = quota::month(&rows, kinds, &caps, &month);
    Ok((month, months))
}
