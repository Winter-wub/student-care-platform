//! Recovery of a round stuck in `sending` (like the roster import's stuck
//! recovery, #45): when no Job of the round is `queued` or `running` any
//! more (a child ended with an error or `lock expired` on its last
//! attempt, or the parent failed every attempt), its `pending` rows can
//! never be sent. They become `failed` ("ran out of attempts") and the
//! round is finished. Run when the detail page loads and by the scheduled
//! Job `announcements.sweep`.

use super::deliver::finish_if_done;
use super::{NAME, store};
use crate::manifest::Job;
use crate::module::JobContext;
use crate::port_types::Statement;
use crate::{Result, Services};

/// The error kind of a row that ran out of attempts.
pub(crate) const RAN_OUT: &str = "ran out of attempts";

/// No Job of the announcement's current round is waiting or running.
/// `a` is the `announcement` alias in the surrounding statement.
const NO_LIVE_JOB: &str = "NOT EXISTS (SELECT 1 FROM job WHERE state IN ('queued', 'running') \
     AND unique_key LIKE ? || ':' || a.id || ':' || a.round || ':%')";

/// Finishes every stuck round (only `id`'s when given); returns how many.
pub(crate) async fn sweep(s: &Services, id: Option<&str>) -> Result<usize> {
    let only = if id.is_some() { " AND a.id = ?" } else { "" };
    let find = Statement::new(format!(
        "SELECT a.id, a.round FROM announcement a WHERE a.status = 'sending'{only} \
         AND {NO_LIVE_JOB} LIMIT 50"
    ));
    let find = match id {
        Some(id) => find.bind(id).bind(NAME),
        None => find.bind(NAME),
    };
    let rows = s.db.query(find).await?;
    for i in 0..rows.len() {
        let id = store::text(&rows, i, "id");
        let round = store::int(&rows, i, "round").unwrap_or(0);
        // Each write re-checks that nothing is still live (a resend moves
        // the round, which the conditions see too).
        let still = |sql: &str| {
            Statement::new(format!(
                "{sql} AND EXISTS (SELECT 1 FROM announcement a WHERE a.id = ? AND a.round = ? \
                 AND a.status = 'sending' AND {NO_LIVE_JOB})"
            ))
        };
        s.db.batch(vec![
            still(
                "UPDATE announcement_delivery SET status = 'failed', error = ? \
                 WHERE announcement_id = ? AND round = ? AND status = 'pending'",
            )
            .bind(RAN_OUT)
            .bind(id.as_str())
            .bind(round)
            .bind(id.as_str())
            .bind(round)
            .bind(NAME),
            // The parent never snapshotted: the round had nothing sent.
            still("UPDATE announcement SET snapshot_round = round WHERE id = ? AND snapshot_round < round")
                .bind(id.as_str())
                .bind(id.as_str())
                .bind(round)
                .bind(NAME),
        ])
        .await?;
        finish_if_done(s, &id, round).await?;
    }
    Ok(rows.len())
}

/// `announcements.sweep` (scheduled): [`sweep`] over every announcement.
pub(crate) async fn job(cx: JobContext, _: Job) -> Result<()> {
    sweep(&cx.services, None).await.map(|_| ())
}
