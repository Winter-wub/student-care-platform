//! Delivery and acknowledgement counts of one announcement, for the
//! detail page and the Mini App.

use super::store;
use crate::port_types::Statement;
use crate::{Deployment, Result};

/// Deliveries of one round on one Channel, by status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RoundStats {
    /// Delivery round (0 for the first send).
    pub round: i64,
    /// Channel kind.
    pub channel: String,
    /// Sent.
    pub sent: i64,
    /// Failed after every retry.
    pub failed: i64,
    /// Skipped (unreachable Link).
    pub skipped: i64,
    /// Not sent yet.
    pub pending: i64,
}

/// Students acknowledged over Students in the Audience, and the
/// per-round, per-Channel delivery counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stats {
    /// Distinct Students with a delivery row (any round, any Channel).
    pub in_audience: i64,
    /// Of those, the ones who acknowledged.
    pub acked: i64,
    /// By round, then Channel.
    pub rounds: Vec<RoundStats>,
}

/// [`Stats`] of announcement `id` (zeros when it has no deliveries).
pub async fn stats(d: &Deployment, id: &str) -> Result<Stats> {
    let db = &d.services().db;
    let totals = db
        .query(
            Statement::new(
                "SELECT count(DISTINCT student_id) AS students, count(DISTINCT CASE WHEN \
                 acked_at IS NOT NULL THEN student_id END) AS acked \
                 FROM announcement_delivery WHERE announcement_id = ?",
            )
            .bind(id),
        )
        .await?;
    let rows = db
        .query(
            Statement::new(
                "SELECT round, channel, \
                 sum(status = 'sent') AS sent, sum(status = 'failed') AS failed, \
                 sum(status = 'skipped') AS skipped, sum(status = 'pending') AS pending \
                 FROM announcement_delivery WHERE announcement_id = ? \
                 GROUP BY round, channel ORDER BY round, channel",
            )
            .bind(id),
        )
        .await?;
    let n = |i: usize, c: &str| store::int(&rows, i, c).unwrap_or(0);
    Ok(Stats {
        in_audience: store::int(&totals, 0, "students").unwrap_or(0),
        acked: store::int(&totals, 0, "acked").unwrap_or(0),
        rounds: (0..rows.len())
            .map(|i| RoundStats {
                round: n(i, "round"),
                channel: store::text(&rows, i, "channel"),
                sent: n(i, "sent"),
                failed: n(i, "failed"),
                skipped: n(i, "skipped"),
                pending: n(i, "pending"),
            })
            .collect(),
    })
}
