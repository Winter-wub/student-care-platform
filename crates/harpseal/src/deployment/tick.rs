//! [`Deployment::tick`]: the per-minute entry point both runtimes call (a
//! Cron Trigger on Workers, a timer natively), and the Deployment's enqueue
//! shortcuts. It enqueues due scheduled Jobs, claims due Jobs with one
//! `UPDATE … RETURNING`, runs them and prunes old rows. The lifecycle is
//! documented in `crate::jobs`.
//!
//! `job.attempts` counts runs started (incremented by the claim) and
//! `job.max_attempts` is the number of **retries** after the first run: with
//! the default 3, a Job runs at most 4 times, retried 1, 5 and 30 minutes
//! after each failure, and fails when a run fails with
//! `attempts > max_attempts`.

use crate::delivery::Timestamp;
use crate::jobs::{JobId, Jobs, cut};
use crate::manifest::Job;
use crate::port_types::{Rows, Statement};
use crate::{Deployment, Result};

/// Longest `last_error` stored.
const MAX_ERROR_CHARS: usize = 500;

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
    /// Expired Staff sessions deleted (#21).
    pub sessions_pruned: u64,
    /// Scheduled Jobs this tick enqueued (0 when another tick in the same
    /// minute already did), the Quota watch (#26) included.
    pub scheduled: usize,
    /// Old `done` and `failed` Job rows deleted.
    pub jobs_pruned: u64,
    /// Webhook event claims (`inbound_event`) older than 7 days deleted,
    /// at most 500 per tick (#35).
    pub events_pruned: u64,
    /// Webhook event claims left unfinished past the 60 s lease and logged
    /// as `inbound.lost`, at most 100 per tick (#44).
    pub events_lost: u64,
}

impl core::fmt::Display for TickReport {
    /// One log line, the same on both runtimes.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "scheduled {}, claimed {}, done {}, retried {}, failed {}, pruned {} jobs and {} sessions",
            self.scheduled,
            self.claimed,
            self.done,
            self.retried,
            self.failed,
            self.jobs_pruned,
            self.sessions_pruned
        )
    }
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
    /// Enqueues Jobs of the registered kinds (see [`Jobs`]).
    pub fn jobs(&self) -> &Jobs {
        &self.queue
    }

    /// [`Jobs::enqueue`]: one Job of `kind`, due at `run_at` (now if `None`).
    /// An unregistered kind is `Error::Invalid`.
    pub async fn enqueue(
        &self,
        kind: &str,
        payload: serde_json::Value,
        run_at: Option<Timestamp>,
    ) -> Result<JobId> {
        self.queue.enqueue(kind, payload, run_at).await
    }

    /// [`Jobs::enqueue_unique`]: `Ok(None)` when a Job with `key` exists.
    pub async fn enqueue_unique(
        &self,
        key: &str,
        kind: &str,
        payload: serde_json::Value,
        run_at: Option<Timestamp>,
    ) -> Result<Option<JobId>> {
        self.queue.enqueue_unique(key, kind, payload, run_at).await
    }

    /// Runs the per-minute work at `now`. First, Staff sessions past their
    /// sliding or absolute limit are deleted, and the scheduled Jobs due in
    /// this minute (or the one before) are enqueued, once per minute
    /// however many ticks run; with `[quotas]` set, the Quota watch
    /// `kernel.usage.watch` is enqueued once per 10-minute window (#26). Then `running` Jobs whose lock
    /// expired (their runtime died mid-run) and that have no retries left
    /// are marked `failed` with `last_error = 'lock expired'`. Then one
    /// statement claims due `queued` Jobs and expired `running` ones that
    /// still have retries, each handler runs, and the outcome is written:
    /// `done`, `queued` again after 1, 5 or 30 minutes, or `failed` once
    /// `attempts > max_attempts`; a handler panic (native) is a failed run.
    /// Last, `done` rows older than `[retention] job_days` and `failed`
    /// rows older than 90 days are deleted, then webhook event claims older
    /// than 7 days (at most 500). Handlers must be idempotent: a
    /// Cron Trigger may fire twice, and an expired lock re-runs a Job.
    pub async fn tick(&self, now: Timestamp) -> Result<TickReport> {
        let db = self.services.db.as_ref();
        let mut report = TickReport::default();
        match crate::auth::prune_sessions(&self.services, now.ms()).await {
            Ok(n) => report.sessions_pruned = n,
            Err(e) => report.errors.push(format!("staff session sweep: {e}")),
        }
        let offset = self.config.utc_offset_minutes().unwrap_or(0);
        match self
            .queue
            .enqueue_scheduled(&self.registry.schedules, now, offset)
            .await
        {
            Ok(n) => report.scheduled = n,
            Err(e) => report.errors.push(format!("scheduled jobs: {e}")),
        }
        // Quota emails (#26): one watch per 10 minutes, only with a Quota.
        if !self.config.quotas.is_empty() {
            match crate::kernel::usage::enqueue_watch(self, now).await {
                Ok(n) => report.scheduled += n,
                Err(e) => report.errors.push(format!("usage watch: {e}")),
            }
        }
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
            "UPDATE job SET state = 'running', lock_token = ?, locked_until = ?, progress = NULL, \
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
            let outcome = match self.run_claimed(c.job, Some(&token)).await {
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
                    .bind(cut(&e.to_string(), MAX_ERROR_CHARS))
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
                    .bind(cut(&e.to_string(), MAX_ERROR_CHARS))
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
        match crate::jobs::prune(&self.services, now, self.config.retention.job_days).await {
            Ok(n) => report.jobs_pruned = n,
            Err(e) => report.errors.push(format!("job prune: {e}")),
        }
        match self.sweep_lost_inbound(now.ms()).await {
            Ok(n) => report.events_lost = n,
            Err(e) => report.errors.push(format!("lost webhook event sweep: {e}")),
        }
        match self.prune_inbound_events(now.ms()).await {
            Ok(n) => report.events_pruned = n,
            Err(e) => report.errors.push(format!("webhook event prune: {e}")),
        }
        Ok(report)
    }
}
