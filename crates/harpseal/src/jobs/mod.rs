//! The Job runner's parts outside the tick itself (#23): the enqueue API
//! ([`Jobs`]), the cron subset ([`cron`]), pruning, the panic guard and the
//! `/admin/jobs.json` status endpoint. `Deployment::tick` (in
//! `deployment/tick.rs`) drives them once a minute.
//!
//! Lifecycle of a `job` row:
//! - **queued** at `run_at` by [`Jobs::enqueue`] / [`Jobs::insert`] (or by
//!   the scheduler for a [`crate::JobSpec::schedule`]);
//! - **running** once a tick claims it with one `UPDATE … RETURNING`: a
//!   fresh lock token, `locked_until = now + 15 min`, `attempts + 1`;
//! - **done** when the handler returns `Ok`;
//! - **queued** again after a failed run while `attempts <= max_attempts`
//!   (`max_attempts` counts *retries*, default 3), `run_at` 1, 5 then 30
//!   minutes later, `last_error` set;
//! - **failed** when a run fails with `attempts > max_attempts` (so at most
//!   4 runs by default), `last_error` kept.
//!
//! A run whose runtime died (Worker CPU limit, process crash) leaves the
//! row `running`; when `locked_until` passes, the next tick re-claims it if
//! retries remain, or marks it `failed` with `last_error = 'lock expired'`.
//! A handler panic on native is caught and counts as a failed run. Every
//! outcome write is conditional on the lock token, so a run whose lock was
//! taken over cannot overwrite the newer run. `done` rows are deleted after
//! `[retention] job_days` (default 30), `failed` rows after 90 days (or
//! `job_days`, if longer).

pub(crate) mod catch;
pub(crate) mod cron;
pub(crate) mod status;

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::delivery::Timestamp;
use crate::port_types::Statement;
use crate::{Error, Result, Services};

/// Days a `failed` Job row is kept (at least `[retention] job_days`).
pub(crate) const KEEP_FAILED_DAYS: u32 = 90;
/// Longest `unique_key` accepted.
const MAX_KEY_CHARS: usize = 200;
const DAY_MS: i64 = 86_400_000;
const MINUTE_MS: i64 = 60_000;

/// `job.id`, a UUIDv7.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub String);

impl core::fmt::Display for JobId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Enqueues Jobs of the kinds the Modules registered. Cheap to clone; get
/// it from [`crate::Deployment::jobs`], [`crate::PageState::jobs`] or
/// [`crate::JobContext::jobs`]. An unregistered kind is
/// [`Error::Invalid`]. Handlers must be idempotent (see the module docs).
#[derive(Clone)]
pub struct Jobs {
    kinds: Arc<BTreeSet<&'static str>>,
    services: Services,
}

impl core::fmt::Debug for Jobs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Jobs").field("kinds", &self.kinds).finish()
    }
}

impl Jobs {
    pub(crate) fn new(kinds: BTreeSet<&'static str>, services: Services) -> Self {
        Self {
            kinds: Arc::new(kinds),
            services,
        }
    }

    /// Whether a Module registered `kind`.
    pub fn knows(&self, kind: &str) -> bool {
        self.kinds.contains(kind)
    }

    /// The `INSERT` for a new Job, to run inside the caller's own `batch`
    /// (all-or-nothing with the caller's other writes). `run_at` defaults
    /// to now.
    pub fn insert(
        &self,
        kind: &str,
        payload: &serde_json::Value,
        run_at: Option<Timestamp>,
    ) -> Result<(JobId, Statement)> {
        self.statement(None, kind, payload, run_at)
    }

    /// Like [`Jobs::insert`] with a dedupe key: the statement inserts
    /// nothing (`rows_affected = 0`) when a Job with `key` exists, in any
    /// state, until that row is pruned. `key` is 1 to 200 characters.
    pub fn insert_unique(
        &self,
        key: &str,
        kind: &str,
        payload: &serde_json::Value,
        run_at: Option<Timestamp>,
    ) -> Result<(JobId, Statement)> {
        if key.is_empty() || key.chars().count() > MAX_KEY_CHARS {
            return Err(Error::Invalid(format!(
                "job unique key must be 1 to {MAX_KEY_CHARS} characters"
            )));
        }
        self.statement(Some(key), kind, payload, run_at)
    }

    fn statement(
        &self,
        key: Option<&str>,
        kind: &str,
        payload: &serde_json::Value,
        run_at: Option<Timestamp>,
    ) -> Result<(JobId, Statement)> {
        if !self.knows(kind) {
            return Err(Error::Invalid(format!("no job kind {kind}")));
        }
        let id = self.services.new_id()?;
        let now = self.services.now_ms();
        let stmt = Statement::new(
            "INSERT INTO job (id, kind, payload, run_at, unique_key, created_at) \
             VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(id.as_str())
        .bind(kind)
        .bind(payload.to_string())
        .bind(run_at.map_or(now, Timestamp::ms))
        .bind(key)
        .bind(now);
        Ok((JobId(id), stmt))
    }

    /// Enqueues one Job of `kind` with `payload`, due at `run_at` (now when
    /// `None`).
    pub async fn enqueue(
        &self,
        kind: &str,
        payload: serde_json::Value,
        run_at: Option<Timestamp>,
    ) -> Result<JobId> {
        let (id, stmt) = self.insert(kind, &payload, run_at)?;
        self.services.db.execute(stmt).await?;
        Ok(id)
    }

    /// Enqueues one Job unless a Job with `key` exists: `Ok(None)` then.
    pub async fn enqueue_unique(
        &self,
        key: &str,
        kind: &str,
        payload: serde_json::Value,
        run_at: Option<Timestamp>,
    ) -> Result<Option<JobId>> {
        let (id, stmt) = self.insert_unique(key, kind, &payload, run_at)?;
        let r = self.services.db.execute(stmt).await?;
        Ok((r.rows_affected > 0).then_some(id))
    }

    /// Enqueues the scheduled Jobs due in the minute holding `now` and in
    /// the minute before (so a tick that slipped past a minute boundary
    /// does not skip it), each once: the key `<kind>@<epoch minute>` makes
    /// a second tick in the same minute a no-op. The payload is
    /// `{"scheduled_at": <minute start, ms>}` and `run_at` the minute
    /// start. Returns how many were new.
    pub(crate) async fn enqueue_scheduled(
        &self,
        schedules: &[(&'static str, cron::Schedule)],
        now: Timestamp,
        offset_minutes: i32,
    ) -> Result<usize> {
        let this = now.ms().div_euclid(MINUTE_MS);
        let mut stmts = Vec::new();
        for minute in [this - 1, this] {
            let start = minute * MINUTE_MS;
            for (kind, s) in schedules {
                if s.is_due(start, offset_minutes) {
                    let payload = serde_json::json!({ "scheduled_at": start });
                    let key = format!("{kind}@{minute}");
                    let (_, stmt) =
                        self.insert_unique(&key, kind, &payload, Some(Timestamp(start)))?;
                    stmts.push(stmt);
                }
            }
        }
        if stmts.is_empty() {
            return Ok(0);
        }
        let results = self.services.db.batch(stmts).await?;
        let new: u64 = results.iter().map(|r| r.rows_affected).sum();
        Ok(usize::try_from(new).unwrap_or(usize::MAX))
    }
}

/// Deletes `done` Jobs finished more than `done_days` ago and `failed`
/// ones more than `max(90, done_days)` days ago. Returns the rows deleted.
pub(crate) async fn prune(services: &Services, now: Timestamp, done_days: u32) -> Result<u64> {
    let failed_days = KEEP_FAILED_DAYS.max(done_days);
    let before = |days: u32| now.ms() - i64::from(days) * DAY_MS;
    let r = services
        .db
        .execute(
            Statement::new(
                "DELETE FROM job WHERE (state = 'done' AND finished_at < ?) \
                 OR (state = 'failed' AND finished_at < ?)",
            )
            .bind(before(done_days))
            .bind(before(failed_days)),
        )
        .await?;
    Ok(r.rows_affected)
}

/// `text` cut to `max` characters (error texts stored in `job`).
pub(crate) fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}
