//! What a send did to each delivery row: the row updates and the error
//! kinds stored (never platform text, which may quote a request).

use super::deliver::Pending;
use super::store;
use crate::port_types::Statement;
use crate::{Error, Result, Services};

/// `job.max_attempts` when the row cannot be read (a run outside a tick).
const DEFAULT_RETRIES: i64 = 3;

/// Unreachable never gets better by retrying.
pub(super) fn is_final(e: &Error) -> bool {
    matches!(e.root(), Error::Unreachable)
}

/// A short error kind for `announcement_delivery.error` (never platform
/// text, which may quote a request).
pub(super) fn kind_of(e: &Error) -> &'static str {
    match e.root() {
        Error::Unreachable => "unreachable",
        Error::RateLimited { .. } => "rate_limited",
        Error::Unsupported(_) => "unsupported",
        Error::Db(_) => "db",
        _ => "platform",
    }
}

pub(super) fn skipped(id: &str) -> Statement {
    Statement::new(
        "UPDATE announcement_delivery SET status = 'skipped', error = 'unreachable' \
         WHERE id = ? AND status = 'pending'",
    )
    .bind(id.to_string())
}

/// Every pending delivery of round `round` on `channel`, skipped as
/// unreachable: the Channel takes no traffic here (dev-only outside dev,
/// or no longer registered, #133), so the round can still finish.
pub(super) fn skipped_channel(id: &str, round: i64, channel: &str) -> Statement {
    Statement::new(
        "UPDATE announcement_delivery SET status = 'skipped', error = 'unreachable' \
         WHERE announcement_id = ? AND round = ? AND channel = ? AND status = 'pending'",
    )
    .bind(id.to_string())
    .bind(round)
    .bind(channel.to_string())
}

/// Retries this Job gets (`job.max_attempts`, 3 by default): a run whose
/// `attempt` is above it is the last.
pub(super) async fn max_retries(s: &Services, job_id: &str) -> Result<u32> {
    let rows =
        s.db.query(Statement::new("SELECT max_attempts FROM job WHERE id = ?").bind(job_id))
            .await?;
    let n = store::int(&rows, 0, "max_attempts").unwrap_or(DEFAULT_RETRIES);
    Ok(u32::try_from(n).unwrap_or(0))
}

pub(super) fn failed(id: &str, kind: &str) -> Statement {
    Statement::new(
        "UPDATE announcement_delivery SET status = 'failed', error = ? WHERE id = ? \
         AND status = 'pending'",
    )
    .bind(kind.to_string())
    .bind(id.to_string())
}

/// The row update for one recipient's result.
pub(super) fn outcome(
    g: &Pending,
    result: &Result<Vec<crate::delivery::SendReceipt>>,
    now: i64,
    last: bool,
) -> Statement {
    match result {
        Ok(receipts) => Statement::new(
            "UPDATE announcement_delivery SET status = 'sent', sent_at = ?, \
             platform_message_id = ?, error = NULL WHERE id = ? AND status = 'pending'",
        )
        .bind(now)
        .bind(
            receipts
                .first()
                .and_then(|r| r.message_id.as_ref())
                .map(|m| m.0.clone()),
        )
        .bind(g.id.clone()),
        Err(e) if is_final(e) => skipped(&g.id),
        Err(e) if last => failed(&g.id, kind_of(e)),
        // Stays pending for the retry; the kind is kept for the page.
        Err(e) => Statement::new(
            "UPDATE announcement_delivery SET error = ? WHERE id = ? AND status = 'pending'",
        )
        .bind(kind_of(e))
        .bind(g.id.clone()),
    }
}
