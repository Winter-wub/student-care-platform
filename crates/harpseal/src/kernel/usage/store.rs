//! Reads `usage_daily` (the only source of every Staff-visible usage
//! number; no platform API is ever called) and the Deployment's Channels.

use super::quota::{self, ChannelMonth, Row};
use crate::port_types::{Statement, Value};
use crate::{Deployment, Result, Services};

/// `usage_daily` rows whose local day is `since` (`YYYY-MM-DD`) or later.
pub(crate) async fn rows_since(services: &Services, since: &str) -> Result<Vec<Row>> {
    let rows = services
        .db
        .query(
            Statement::new(
                "SELECT channel, day, metered, free, failed FROM usage_daily WHERE day >= ? \
                 ORDER BY day, channel",
            )
            .bind(since),
        )
        .await?;
    let text = |i: usize, c: &str| {
        rows.get(i, c)
            .and_then(Value::as_text)
            .unwrap_or_default()
            .to_string()
    };
    let int = |i: usize, c: &str| rows.get(i, c).and_then(Value::as_i64).unwrap_or(0);
    Ok((0..rows.len())
        .map(|i| Row {
            channel: text(i, "channel"),
            day: text(i, "day"),
            metered: int(i, "metered"),
            free: int(i, "free"),
            failed: int(i, "failed"),
        })
        .collect())
}

/// The Deployment's Channel kinds, in registration order.
pub(crate) fn kinds(d: &Deployment) -> Vec<String> {
    d.channel_kinds()
        .iter()
        .map(|k| k.as_str().to_string())
        .collect()
}

/// The current local month (`YYYY-MM`) and each Channel in it.
pub(crate) async fn this_month(d: &Deployment) -> Result<(String, Vec<ChannelMonth>)> {
    this_month_of(d, &kinds(d)).await
}

/// The current local month (`YYYY-MM`) and each of `kinds` in it, from one
/// read.
pub(crate) async fn this_month_of(
    d: &Deployment,
    kinds: &[String],
) -> Result<(String, Vec<ChannelMonth>)> {
    let offset = d.config().utc_offset_minutes().unwrap_or(0);
    let month = quota::month_at(d.services().now_ms(), offset);
    let rows = rows_since(d.services(), &format!("{month}-01")).await?;
    let caps = super::package::caps(d.services(), d.config()).await?;
    let months = quota::month(&rows, kinds, &caps, &month);
    Ok((month, months))
}
