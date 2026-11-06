//! [`Deployment::tick`]: the per-minute entry point both runtimes call (a
//! Cron Trigger on Workers, a timer natively). It claims due Jobs with one
//! `UPDATE … RETURNING` and runs them. #23 owns the full Job lifecycle
//! (scheduled Jobs, pruning); this is the claim loop it builds on.
//!
//! `job.attempts` counts runs started (incremented by the claim) and
//! `job.max_attempts` is the number of **retries** after the first run: with
//! the default 3, a Job runs at most 4 times, retried 1, 5 and 30 minutes
//! after each failure, and fails when a run fails with
//! `attempts > max_attempts`.

use crate::delivery::Timestamp;
use crate::manifest::Job;
use crate::port_types::{Rows, Statement};
use crate::{Deployment, Result};

/// Jobs claimed per tick.
const CLAIM_LIMIT: i64 = 25;
/// How long a claim holds before another tick may re-claim it.
const LOCK_MS: i64 = 15 * 60_000;
/// Retry delays after the 1st, 2nd and 3rd (or later) failed run.
const BACKOFF_MS: [i64; 3] = [60_000, 5 * 60_000, 30 * 60_000];

/// What one [`Deployment::tick`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TickReport {
    /// Jobs claimed (due, or with an expired lock and retries left).
    pub claimed: usize,
    /// Jobs whose handler succeeded.
    pub done: usize,
    /// Jobs that failed and will be retried.
    pub retried: usize,
    /// Jobs that failed for the last time (including expired locks with no
    /// retries left, which are not run again).
    pub failed: usize,
    /// Outcome writes that failed (the Job keeps its lock and is re-claimed
    /// when it expires). The runtime logs these.
    pub errors: Vec<String>,
}

/// One claimed row.
struct Claimed {
    job: Job,
    attempts: i64,
    max_retries: i64,
}

fn claimed(rows: &Rows) -> Vec<Claimed> {
    let mut out: Vec<Claimed> = (0..rows.len())
        .map(|i| {
            let text = |c: &str| {
                rows.get(i, c)
                    .and_then(|v| v.as_text())
                    .unwrap_or_default()
                    .to_string()
            };
            let int = |c: &str| rows.get(i, c).and_then(|v| v.as_i64()).unwrap_or(0);
            let attempts = int("attempts");
            Claimed {
                job: Job {
                    id: text("id"),
                    kind: text("kind"),
                    payload: text("payload"),
                    attempt: u32::try_from(attempts).unwrap_or(u32::MAX),
                },
                attempts,
                max_retries: int("max_attempts"),
            }
        })
        .collect();
    // RETURNING order is unspecified; run in id order (UUIDv7 = creation).
    out.sort_by(|a, b| a.job.id.cmp(&b.job.id));
    out
}

impl Deployment {
    /// Runs the per-minute work at `now`. First, `running` Jobs whose lock
    /// expired (their runtime died mid-run) and that have no retries left
    /// are marked `failed` with `last_error = 'lock expired'`. Then one
    /// statement claims due `queued` Jobs and expired `running` ones that
    /// still have retries, each handler runs, and the outcome is written:
    /// `done`, `queued` again after 1, 5 or 30 minutes, or `failed` once
    /// `attempts > max_attempts`. Handlers must be idempotent: a Cron
    /// Trigger may fire twice, and an expired lock re-runs a Job.
    pub async fn tick(&self, now: Timestamp) -> Result<TickReport> {
        let db = self.services.db.as_ref();
        let mut report = TickReport::default();
        let expired = db
            .execute(
                Statement::new(
                    "UPDATE job SET state = 'failed', finished_at = ?, lock_token = NULL, \
                     locked_until = NULL, last_error = 'lock expired' \
                     WHERE state = 'running' AND locked_until < ? AND attempts > max_attempts",
                )
                .bind(now.ms())
                .bind(now.ms()),
            )
            .await?;
        report.failed += usize::try_from(expired.rows_affected).unwrap_or(usize::MAX);

        let token = self.services.random_hex(16)?;
        let claim = Statement::new(
            "UPDATE job SET state = 'running', lock_token = ?, locked_until = ?, \
             attempts = attempts + 1 WHERE id IN (SELECT id FROM job WHERE \
             (state = 'queued' AND run_at <= ?) \
             OR (state = 'running' AND locked_until < ? AND attempts <= max_attempts) \
             ORDER BY run_at, id LIMIT ?) \
             RETURNING id, kind, payload, attempts, max_attempts",
        )
        .bind(token.as_str())
        .bind(now.plus_ms(LOCK_MS).ms())
        .bind(now.ms())
        .bind(now.ms())
        .bind(CLAIM_LIMIT);
        let jobs = claimed(&db.query(claim).await?);
        report.claimed = jobs.len();

        for c in jobs {
            let id = c.job.id.clone();
            let outcome = match self.run_job(c.job).await {
                Ok(()) => {
                    report.done += 1;
                    Statement::new(
                        "UPDATE job SET state = 'done', finished_at = ?, lock_token = NULL, \
                         locked_until = NULL, last_error = NULL WHERE id = ? AND lock_token = ?",
                    )
                    .bind(now.ms())
                }
                Err(e) if c.attempts > c.max_retries => {
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
                    let step = usize::try_from(c.attempts - 1).unwrap_or(0);
                    let delay = BACKOFF_MS[step.min(BACKOFF_MS.len() - 1)];
                    Statement::new(
                        "UPDATE job SET state = 'queued', run_at = ?, lock_token = NULL, \
                         locked_until = NULL, last_error = ? WHERE id = ? AND lock_token = ?",
                    )
                    .bind(now.plus_ms(delay).ms())
                    .bind(e.to_string())
                }
            };
            if let Err(e) = db
                .execute(outcome.bind(id.as_str()).bind(token.as_str()))
                .await
            {
                report
                    .errors
                    .push(format!("job {id}: outcome not saved: {e}"));
            }
        }
        Ok(report)
    }
}
