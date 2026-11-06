//! The Job `kernel.roster.import`. Payload `{import_id, chunk}`: applies
//! rows `[chunk, chunk + 500)` of the stored file in one `Db::batch` and
//! queues the next step in the same batch (`chunk.rs`). Payload
//! `{import_id, chunk, left: n}`: left batch `n` of a `full` import
//! (`finish.rs`). Each run is small, so it fits a Worker's limits.
//!
//! Idempotent: each run re-reads and re-parses the stored file, and each
//! batch claims its step with a compare-and-set that every other statement
//! of the batch is gated on (through a per-run token in `counts.run`), so
//! a repeated run of a finished step writes nothing.
//! Every Student the import listed gets `roster_imported_at` = the
//! import's `started_at`, which the claim makes later than any earlier
//! import's, so "not listed" is `roster_imported_at < started_at`.

use serde::Deserialize;
use serde_json::json;

use super::service::{JOB_KIND, job_key};
use super::store;
use crate::manifest::Job;
use crate::module::JobContext;
use crate::port_types::{Statement, Value};
use crate::{Error, Result, Services};

#[derive(Debug, Deserialize)]
struct Payload {
    import_id: String,
    #[serde(default)]
    chunk: i64,
    /// Set on the left phase's Jobs: the batch number.
    #[serde(default)]
    left: Option<i64>,
}

/// The handler registered for [`JOB_KIND`]. After its last attempt fails,
/// the import is marked `failed` (the reason never quotes the file).
pub(crate) async fn run(cx: JobContext, job: Job) -> Result<()> {
    let p: Payload = serde_json::from_str(&job.payload)
        .map_err(|_| Error::Invalid("roster import job payload".into()))?;
    let outcome = step(&cx, &job, &p).await;
    if let Err(e) = &outcome
        && last_attempt(&cx.services, &job).await
    {
        let failure = store::Failure {
            job: Some(describe(e)),
            ..store::Failure::default()
        };
        let stmt = Statement::new(
            "UPDATE roster_import SET state = 'failed', error = ?, finished_at = ? \
             WHERE id = ? AND state IN ('queued', 'running')",
        )
        .bind(serde_json::to_string(&failure).unwrap_or_default())
        .bind(cx.services.now_ms())
        .bind(p.import_id.as_str());
        let _ = cx.services.db.execute(stmt).await;
    }
    outcome
}

async fn step(cx: &JobContext, job: &Job, p: &Payload) -> Result<()> {
    let s = &cx.services;
    let Some(imp) = store::load(s, &p.import_id).await? else {
        return Ok(());
    };
    if !matches!(imp.state.as_str(), "queued" | "running") {
        return Ok(());
    }
    if let Some(n) = p.left {
        return super::finish::left_batch(cx, job, &imp, n).await;
    }
    if imp.applied_rows > p.chunk && imp.applied_rows < imp.total_rows {
        // This chunk is applied; make sure the next one is queued.
        let next = imp.applied_rows;
        let payload = json!({ "import_id": imp.id, "chunk": next });
        cx.jobs
            .enqueue_unique(&job_key(&imp.id, next), JOB_KIND, payload, None)
            .await?;
        return Ok(());
    }
    if imp.applied_rows < p.chunk.min(imp.total_rows) {
        return Err(Error::Invalid(
            "roster chunk ahead of the applied rows".into(),
        ));
    }
    let claim = Statement::new(
        "UPDATE roster_import SET state = 'running', started_at = COALESCE(started_at, \
         MAX(?, (SELECT COALESCE(MAX(roster_imported_at), 0) + 1 FROM student))) \
         WHERE id = ? AND state IN ('queued', 'running') AND applied_rows = ?",
    )
    .bind(s.now_ms())
    .bind(imp.id.as_str())
    .bind(imp.applied_rows);
    if s.db.execute(claim).await?.rows_affected == 0 {
        return Ok(());
    }
    let Some(imp) = store::load(s, &p.import_id).await? else {
        return Ok(());
    };
    if imp.applied_rows < imp.total_rows {
        if super::chunk::apply(cx, &imp).await? {
            let applied = (imp.applied_rows + super::chunk::CHUNK).min(imp.total_rows);
            let note = format!("{applied} / {}", imp.total_rows);
            cx.progress(&note).await?;
        }
        return Ok(());
    }
    // Every row is applied (a re-run of the last chunk, or a file with no
    // rows): make sure the next step is queued, or the import is done.
    let next = super::chunk::complete(cx, &imp, None)?;
    s.db.batch(next).await?;
    Ok(())
}

async fn last_attempt(s: &Services, job: &Job) -> bool {
    let stmt = Statement::new("SELECT max_attempts FROM job WHERE id = ?").bind(job.id.as_str());
    let max =
        s.db.query(stmt)
            .await
            .ok()
            .and_then(|r| r.get(0, "max_attempts").and_then(Value::as_i64));
    max.is_some_and(|m| i64::from(job.attempt) > m)
}

/// The failure as Staff see it: the Kernel's own messages as they are,
/// other errors by kind only (a database or platform message could quote
/// a value from the file).
pub(crate) fn describe(e: &Error) -> String {
    match e.root() {
        Error::Invalid(m) => m.clone(),
        Error::Db(_) => "database error".into(),
        Error::Conflict(_) => "conflicting change".into(),
        _ => "platform error".into(),
    }
}
