//! Job `kernel.backup.run`. Its payload says which part it is:
//!
//! - `{"scheduled_at": ms}` (the cron) or `{"manual": staff id}` ("back up
//!   now"): starts a run, unless one is running: inserts its row, its
//!   first step and its watch in one batch;
//! - `{"run": id}`: step `n` of run `id` (unique key
//!   `kernel.backup.step:<id>:<n>`), enqueued by the start (step 0) or by
//!   the step before. A failed step is retried as itself;
//! - `{"watch": id}`: two hours after a run started, marks it `failed` if
//!   it has not moved for two hours (its Worker died mid-step and every
//!   retry did too), else looks again two hours later.
//!
//! Every step resumes from the saved progress object, so a re-run after a
//! crash rewrites the same entries. A step of a run that is not `running`
//! (finished, failed, or a restored copy's) does nothing.

use serde_json::{Value as Json, json};

use super::notify::{self, Failure};
use super::store::{self, Run};
use super::{KIND, PREFIX, progress_key, prune, stamp};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::backup::FileStoreBackup;
use crate::backup::progress::{Budget, DumpProgress, Step};
use crate::delivery::Timestamp;
use crate::manifest::Job;
use crate::port_types::{Statement, Value};
use crate::{Deployment, Error, JobContext, Result};

/// What one Job run may dump before it saves and hands over (well inside
/// a Worker's CPU, memory and subrequest limits), and the largest table it
/// accepts (`[backup] max_table_mb`, `max_table_rows`).
///
/// **Subrequests.** A Worker invocation may make 1,000, and one tick runs
/// up to 25 Jobs in a single invocation. A step stops before a table or
/// object would take its own queries past [`STEP_QUERIES`]; with the run's
/// bookkeeping (about ten more) and, on the first step only, the schema
/// read (two per table), a step stays near 400, leaving at least 550 for
/// the other Jobs of its tick.
fn budget(d: &Deployment) -> Budget {
    let b = &d.config().backup;
    Budget {
        rows: 50_000,
        bytes: 32 * 1024 * 1024,
        files: 200,
        queries: STEP_QUERIES,
        max_table_rows: b.max_table_rows,
        max_table_bytes: u64::from(b.max_table_mb) * 1024 * 1024,
        pause_before_files: true,
    }
}
/// Database queries and FileStore calls one step may make (see [`budget`]).
const STEP_QUERIES: u64 = 350;

/// A run that has not moved for this long is failed by its watch.
pub(super) const STALL_MS: i64 = 2 * 3_600_000;

pub(super) async fn run(cx: JobContext, job: Job) -> Result<()> {
    let d = cx.deployment.clone();
    let payload: Json = serde_json::from_str(&job.payload).unwrap_or(Json::Null);
    if let Some(id) = payload["watch"].as_str() {
        return watch(&d, id).await;
    }
    let Some(id) = payload["run"].as_str().map(String::from) else {
        // A start: retried, it finds its own run going and does nothing.
        return begin(&d, payload["manual"].as_str()).await;
    };
    let Err(e) = step(&d, &id, &job).await else {
        return Ok(());
    };
    let last = last_attempt(&d, &job).await;
    let s = d.services();
    let done = store::get(s, &id)
        .await
        .ok()
        .flatten()
        .is_some_and(|r| r.state == "done");
    if done {
        // The backup is fine; retention after it failed.
        let _ = store::note_prune_error(s, &id, Some(&e.to_string())).await;
    } else {
        // A running run keeps the error, and fails on the last attempt.
        let _ = store::note_error(s, &id, &e.to_string(), last).await;
    }
    if last {
        let kind = if done {
            Failure::Prune
        } else {
            Failure::Backup
        };
        notify::failure(&d, kind).await;
    }
    Err(e)
}

/// Whether this run of `job` is its last (`attempts > max_attempts`).
async fn last_attempt(d: &Deployment, job: &Job) -> bool {
    let rows = d
        .services()
        .db
        .query(Statement::new("SELECT max_attempts FROM job WHERE id = ?").bind(job.id.as_str()))
        .await;
    let max = rows
        .ok()
        .and_then(|r| r.get(0, "max_attempts").and_then(Value::as_i64))
        .unwrap_or(3);
    i64::from(job.attempt) > max
}

/// Starts a run: fails a stalled one first, then inserts the row (only
/// while none is running), its first step and its watch in one batch.
/// Does nothing while another run is still going.
async fn begin(d: &Deployment, by: Option<&str>) -> Result<()> {
    let s = d.services();
    if let Some(r) = store::running(s).await?
        && !stalled(d, &r).await?
    {
        return Ok(());
    }
    let id = s.new_id()?;
    let prefix = format!("{PREFIX}{}/", stamp(s.now_ms()));
    let origin = if by.is_some() { "manual" } else { "schedule" };
    let row = store::insert(s, &id, &prefix, origin, by);
    let exists =
        || Statement::new("EXISTS (SELECT 1 FROM backup_run WHERE id = ?)").bind(id.as_str());
    let (_, first) = d.jobs().insert_unique_if(
        &format!("kernel.backup.step:{id}:0"),
        KIND,
        &json!({ "run": id }),
        None,
        exists(),
    )?;
    let (_, watch) = d.jobs().insert_unique_if(
        &format!("kernel.backup.watch:{id}:0"),
        KIND,
        &json!({ "watch": id }),
        Some(Timestamp(s.now_ms() + STALL_MS)),
        exists(),
    )?;
    s.db.batch(vec![row, first, watch]).await?;
    Ok(())
}

/// Fails `r` when it has not moved for [`STALL_MS`] (and emails); `true`
/// when it did.
async fn stalled(d: &Deployment, r: &Run) -> Result<bool> {
    let s = d.services();
    if s.now_ms() - r.updated_at < STALL_MS {
        return Ok(false);
    }
    let failed = store::note_error(s, &r.id, "stalled: no progress for two hours", true).await?;
    if failed {
        notify::failure(d, Failure::Backup).await;
    }
    Ok(true)
}

async fn watch(d: &Deployment, id: &str) -> Result<()> {
    let s = d.services();
    let Some(r) = store::get(s, id).await? else {
        return Ok(());
    };
    if r.state != "running" || stalled(d, &r).await? {
        return Ok(());
    }
    // Still moving: look again later.
    let at = s.now_ms() + STALL_MS;
    let key = format!("kernel.backup.watch:{id}:{at}");
    d.enqueue_unique(&key, KIND, json!({ "watch": id }), Some(Timestamp(at)))
        .await?;
    Ok(())
}

/// One step of run `id`; on the last, finishes it and applies retention
/// (again on a retry, if that is what failed).
async fn step(d: &Deployment, id: &str, job: &Job) -> Result<()> {
    let s = d.services();
    let Some(r) = store::get(s, id).await? else {
        return Ok(());
    };
    match r.state.as_str() {
        "running" => {}
        "done" if job.attempt > 1 => return prune_after(d, &r, job).await,
        _ => return Ok(()),
    }
    let mut sink = FileStoreBackup::new(s.files.clone(), r.prefix.clone());
    let key = progress_key(&r.prefix);
    let mut progress = match s.files.get(&key).await? {
        Some(raw) => serde_json::from_slice::<DumpProgress>(&raw)
            .map_err(|e| Error::Invalid(format!("backup progress {key}: {e}")))?,
        None if r.progress == 0 => {
            // Saved before any entry is written: a failed run can always
            // be deleted entry by entry.
            let p = DumpProgress::start(d).await?;
            save(d, &key, &p).await?;
            p
        }
        None => return Err(Error::Invalid(format!("backup progress {key} is missing"))),
    };
    match progress.step(d, &mut sink, budget(d)).await? {
        Step::More => {
            save(d, &key, &progress).await?;
            if store::advance(s, id, progress.bytes).await? {
                let next = format!("kernel.backup.step:{id}:{}", r.progress + 1);
                d.enqueue_unique(&next, KIND, json!({ "run": id }), None)
                    .await?;
            }
            Ok(())
        }
        Step::Done(report) => {
            let m = &report.manifest;
            let rows: u64 = m.tables.iter().map(|t| t.rows).sum();
            let files = m.files.len() - report.missing_files.len();
            let now = s.now_ms();
            let bytes = i64::try_from(report.bytes).unwrap_or(i64::MAX);
            let done = Statement::new(
                "UPDATE backup_run SET state = 'done', finished_at = ?, updated_at = ?, \
                 progress = progress + 1, bytes = ?, table_count = ?, row_count = ?, \
                 file_count = ?, manifest_sha256 = ?, error = NULL \
                 WHERE id = ? AND state = 'running'",
            )
            .bind(now)
            .bind(now)
            .bind(bytes)
            .bind(m.tables.len() as i64)
            .bind(i64::try_from(rows).unwrap_or(i64::MAX))
            .bind(files as i64)
            .bind(report.manifest_sha256.as_str())
            .bind(id);
            let after = json!({
                "prefix": r.prefix, "tables": m.tables.len(), "rows": rows, "files": files,
                "missing_files": report.missing_files.len(), "bytes": report.bytes,
                "manifest_sha256": report.manifest_sha256, "origin": r.origin,
            });
            let target = Target::new("backup_run", id);
            let audit = audit::entry(
                s,
                &Actor::Job(job.id.clone()),
                "backup.create",
                Some(&target),
                None,
                Some(after),
                &RequestCtx::default(),
            )?;
            s.db.batch(vec![done, audit]).await?;
            // Only a hint for a resume that will not come: never fatal.
            let _ = s.files.delete(&key).await;
            prune_after(d, &r, job).await
        }
    }
}

async fn save(d: &Deployment, key: &str, p: &DumpProgress) -> Result<()> {
    let raw = serde_json::to_vec(p).map_err(|e| Error::Invalid(format!("backup progress: {e}")))?;
    d.services().files.put(key, raw, "application/json").await
}

/// Retention after done run `r`; a success clears an earlier retention
/// error on it.
async fn prune_after(d: &Deployment, r: &Run, job: &Job) -> Result<()> {
    prune::expired(d, job).await?;
    if r.prune_error.is_some() {
        store::note_prune_error(d.services(), &r.id, None).await?;
    }
    Ok(())
}
