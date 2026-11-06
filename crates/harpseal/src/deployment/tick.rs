//! [`Deployment::tick`]: the per-minute entry point both runtimes call (a
//! Cron Trigger on Workers, a timer natively). It claims due Jobs with one
//! UPDATE and runs them. #23 owns the full Job lifecycle (scheduled Jobs,
//! pruning); this is the claim loop it builds on.

use crate::delivery::Timestamp;
use crate::manifest::Job;
use crate::port_types::Statement;
use crate::{Deployment, Result};

/// Jobs claimed per tick.
const CLAIM_LIMIT: i64 = 25;
/// How long a claim holds before another tick may re-claim it.
const LOCK_MS: i64 = 15 * 60_000;
/// Retry delays after the 1st, 2nd and later failures (1, 5, 30 minutes).
const BACKOFF_MS: [i64; 3] = [60_000, 5 * 60_000, 30 * 60_000];

/// What one [`Deployment::tick`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TickReport {
    /// Jobs claimed (due, or with a stale lock).
    pub claimed: usize,
    /// Jobs whose handler succeeded.
    pub done: usize,
    /// Jobs that failed and will be retried.
    pub retried: usize,
    /// Jobs that failed for the last time.
    pub failed: usize,
}

impl Deployment {
    /// Runs the per-minute work at `now`: claims due `queued` Jobs (and
    /// `running` ones whose lock expired), runs each with its handler and
    /// records the outcome. Failures retry after 1, 5 and 30 minutes up to
    /// `max_attempts`. Handlers must be idempotent: a Cron Trigger may fire
    /// twice.
    pub async fn tick(&self, now: Timestamp) -> Result<TickReport> {
        let db = self.services.db.as_ref();
        let token = self.services.random_hex(16)?;
        let claim = Statement::new(
            "UPDATE job SET state = 'running', lock_token = ?, locked_until = ?, \
             attempts = attempts + 1 WHERE id IN (SELECT id FROM job WHERE \
             (state = 'queued' AND run_at <= ?) OR (state = 'running' AND locked_until < ?) \
             ORDER BY run_at, id LIMIT ?)",
        )
        .bind(token.as_str())
        .bind(now.plus_ms(LOCK_MS).ms())
        .bind(now.ms())
        .bind(now.ms())
        .bind(CLAIM_LIMIT);
        db.execute(claim).await?;
        let rows = db
            .query(
                Statement::new(
                    "SELECT id, kind, payload, attempts, max_attempts FROM job \
                     WHERE lock_token = ? ORDER BY run_at, id",
                )
                .bind(token.as_str()),
            )
            .await?;
        let mut report = TickReport {
            claimed: rows.len(),
            ..TickReport::default()
        };
        for i in 0..rows.len() {
            let text = |c: &str| {
                rows.get(i, c)
                    .and_then(|v| v.as_text())
                    .unwrap_or_default()
                    .to_string()
            };
            let int = |c: &str| rows.get(i, c).and_then(|v| v.as_i64()).unwrap_or(0);
            let (attempts, max) = (int("attempts"), int("max_attempts"));
            let job = Job {
                id: text("id"),
                kind: text("kind"),
                payload: text("payload"),
                attempt: u32::try_from(attempts).unwrap_or(u32::MAX),
            };
            let id = job.id.clone();
            let outcome = match self.run_job(job).await {
                Ok(()) => {
                    report.done += 1;
                    Statement::new(
                        "UPDATE job SET state = 'done', finished_at = ?, lock_token = NULL, \
                         locked_until = NULL, last_error = NULL WHERE id = ? AND lock_token = ?",
                    )
                    .bind(now.ms())
                }
                Err(e) if attempts >= max => {
                    report.failed += 1;
                    Statement::new(
                        "UPDATE job SET state = 'failed', finished_at = ?, lock_token = NULL, \
                         locked_until = NULL, last_error = ? WHERE id = ? AND lock_token = ?",
                    )
                    .bind(now.ms())
                    .bind(e.to_string())
                }
                Err(e) => {
                    report.retried += 1;
                    let step = usize::try_from(attempts - 1).unwrap_or(0);
                    let delay = BACKOFF_MS[step.min(BACKOFF_MS.len() - 1)];
                    Statement::new(
                        "UPDATE job SET state = 'queued', run_at = ?, lock_token = NULL, \
                         locked_until = NULL, last_error = ? WHERE id = ? AND lock_token = ?",
                    )
                    .bind(now.plus_ms(delay).ms())
                    .bind(e.to_string())
                }
            };
            db.execute(outcome.bind(id).bind(token.as_str())).await?;
        }
        Ok(report)
    }
}
