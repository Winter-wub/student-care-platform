//! Confirming and cancelling a preview. Each is one `Db::batch` gated on
//! its own audit row: the row is written only while the import is still
//! `previewed`, and the state change runs only if that row exists.

use serde_json::json;

use super::store::{self, Import};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::config::RosterConfig;
use crate::jobs::Jobs;
use crate::port_types::Statement;
use crate::{Result, Services};

/// The Job kind that applies an import.
pub(crate) const JOB_KIND: &str = "kernel.roster.import";

/// What a confirm or cancel did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The change went through.
    Done,
    /// It is no longer `previewed`: confirmed, cancelled or replaced
    /// already (a double submit lands here). Carries the current state.
    NotPreviewed(String),
    /// No such import.
    Missing,
    /// Its file was deleted a year after upload (`prune.rs`): nothing
    /// left to import.
    FileDeleted,
    /// Confirm only: the preview's mode is no longer in `[roster] modes`
    /// (the Deployment's config changed after the upload). Nothing is
    /// queued; the preview stays until cancelled, replaced or pruned.
    ModeNotAllowed,
}

/// The unique key of chunk Job `offset` of import `id`: the first chunk
/// is `roster:<id>`, the next ones `roster:<id>:<offset>`.
pub(crate) fn job_key(id: &str, offset: i64) -> String {
    if offset == 0 {
        format!("roster:{id}")
    } else {
        format!("roster:{id}:{offset}")
    }
}

/// The unique key of left batch `n` of import `id`.
pub(crate) fn left_key(id: &str, n: i64) -> String {
    format!("roster:{id}:left:{n}")
}

fn previewed(id: &str) -> Statement {
    Statement::new("EXISTS (SELECT 1 FROM roster_import WHERE id = ? AND state = 'previewed')")
        .bind(id.to_string())
}

/// `previewed` → `queued`, audit `roster.import` (after = counts, file
/// hash, mode, file name) and the first chunk Job, in one batch. Refused
/// ([`Outcome::ModeNotAllowed`]) when `roster` no longer allows the
/// preview's mode: a `full` preview made before a Deployment went
/// `upsert`-only must not mark the other cohorts left (#79). An import
/// already queued or running is not asked again.
pub(crate) async fn confirm(
    s: &Services,
    roster: &RosterConfig,
    jobs: &Jobs,
    id: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<Outcome> {
    let Some(imp) = store::load(s, id).await? else {
        return Ok(Outcome::Missing);
    };
    if imp.state != "previewed" {
        return Ok(Outcome::NotPreviewed(imp.state));
    }
    if imp.file_key.is_empty() {
        return Ok(Outcome::FileDeleted);
    }
    if !roster.allows_mode(&imp.mode) {
        return Ok(Outcome::ModeNotAllowed);
    }
    let after = json!({
        "file_name": imp.file_name,
        "mode": imp.mode,
        "sha256": imp.counts.sha256,
        "total_rows": imp.total_rows,
        "counts": imp.counts.preview,
    });
    let (audit, gate) = audit::entry_if(
        s,
        actor,
        "roster.import",
        Some(&Target::new("roster_import", id)),
        Some(json!({ "state": "previewed" })),
        Some(after),
        ctx,
        previewed(id),
    )?;
    let queue = Statement::new(
        "UPDATE roster_import SET state = 'queued' WHERE id = ? AND state = 'previewed' \
         AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
    )
    .bind(id)
    .bind(gate);
    // The key makes a second submit's insert a no-op; a Job for an import
    // that lost the race to a cancel finds it `cancelled` and stops.
    let (_, job) = jobs.insert_unique(
        &job_key(id, 0),
        JOB_KIND,
        &json!({ "import_id": id, "chunk": 0 }),
        None,
    )?;
    let done = s.db.batch(vec![audit, queue, job]).await?;
    settle(s, &imp, done.get(1).map(|r| r.rows_affected)).await
}

/// `previewed` → `cancelled`, audit `roster.import.cancel`.
pub(crate) async fn cancel(
    s: &Services,
    id: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<Outcome> {
    let Some(imp) = store::load(s, id).await? else {
        return Ok(Outcome::Missing);
    };
    if imp.state != "previewed" {
        return Ok(Outcome::NotPreviewed(imp.state));
    }
    let (audit, gate) = audit::entry_if(
        s,
        actor,
        "roster.import.cancel",
        Some(&Target::new("roster_import", id)),
        Some(json!({ "state": "previewed" })),
        Some(json!({ "state": "cancelled" })),
        ctx,
        previewed(id),
    )?;
    let change = Statement::new(
        "UPDATE roster_import SET state = 'cancelled', finished_at = ? \
         WHERE id = ? AND state = 'previewed' AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
    )
    .bind(s.now_ms())
    .bind(id)
    .bind(gate);
    let done = s.db.batch(vec![audit, change]).await?;
    settle(s, &imp, done.get(1).map(|r| r.rows_affected)).await
}

async fn settle(s: &Services, imp: &Import, changed: Option<u64>) -> Result<Outcome> {
    if changed == Some(1) {
        return Ok(Outcome::Done);
    }
    let state = store::load(s, &imp.id)
        .await?
        .map_or_else(|| imp.state.clone(), |i| i.state);
    Ok(Outcome::NotPreviewed(state))
}
