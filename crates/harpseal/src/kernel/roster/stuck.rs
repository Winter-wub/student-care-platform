//! Imports whose Job died. A run killed by the Workers CPU limit (the
//! runner marks its Job failed with "lock expired"), a panic or an
//! aborted isolate never reaches the handler's own failure path, so the
//! import would stay `queued` / `running` and hold the one-import slot
//! forever. [`reap`] marks such an import `failed` once no Job of it is
//! `queued` or `running` any more; uploads and the import page call it.
//! Staff can also mark an import failed by hand once it is older than the
//! Job lock window ([`mark_failed`], audited `roster.import.fail`).

use serde_json::json;

use super::service::Outcome;
use super::store::{self, Failure, Import};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::port_types::{Statement, Value};
use crate::{Result, Services};

/// How long an import must have been queued or running before Staff may
/// mark it failed: the Job runner's lock (15 minutes).
pub(crate) const STUCK_AFTER_MS: i64 = 15 * 60_000;

/// SQL matching the import's Jobs (two parameters: the key and the key
/// prefix pattern).
const ITS_JOBS: &str = "(unique_key = ? OR unique_key LIKE ?)";

fn bind_keys(st: Statement, id: &str) -> Statement {
    st.bind(format!("roster:{id}"))
        .bind(format!("roster:{id}:%"))
}

/// Marks `imp` failed when it is `queued` / `running` and none of its
/// Jobs is `queued` or `running`; `true` when it did. The reason names
/// what happened to the Job, never its error text.
pub(crate) async fn reap(s: &Services, imp: &Import) -> Result<bool> {
    if !matches!(imp.state.as_str(), "queued" | "running") {
        return Ok(false);
    }
    let latest = bind_keys(
        Statement::new(format!(
            "SELECT state, last_error FROM job WHERE {ITS_JOBS} \
             ORDER BY created_at DESC, id DESC LIMIT 1"
        )),
        &imp.id,
    );
    let rows = s.db.query(latest).await?;
    let text = |c: &str| rows.get(0, c).and_then(Value::as_text).map(str::to_string);
    let reason = match (text("state").as_deref(), text("last_error").as_deref()) {
        (Some("queued" | "running"), _) => return Ok(false),
        (None, _) => "the import Job is missing",
        (Some("failed"), Some("lock expired")) => "the import Job ran out of time",
        (Some("failed"), _) => "the import Job failed after its retries",
        _ => "the import Job stopped before the import finished",
    };
    let failure = Failure {
        job: Some(reason.into()),
        ..Failure::default()
    };
    let stmt = bind_keys(
        Statement::new(format!(
            "UPDATE roster_import SET state = 'failed', error = ?, finished_at = ? \
             WHERE id = ? AND state IN ('queued', 'running') AND NOT EXISTS \
             (SELECT 1 FROM job WHERE {ITS_JOBS} AND state IN ('queued', 'running'))"
        ))
        .bind(serde_json::to_string(&failure).unwrap_or_default())
        .bind(s.now_ms())
        .bind(imp.id.as_str()),
        &imp.id,
    );
    Ok(s.db.execute(stmt).await?.rows_affected == 1)
}

/// [`reap`] for every import holding the slot.
pub(crate) async fn reap_busy(s: &Services) -> Result<()> {
    for imp in store::recent(s, 20).await? {
        reap(s, &imp).await?;
    }
    Ok(())
}

/// Whether Staff may mark `imp` failed at `now`.
pub(crate) fn may_mark_failed(imp: &Import, now: i64) -> bool {
    matches!(imp.state.as_str(), "queued" | "running")
        && imp.started_at.unwrap_or(imp.created_at) < now - STUCK_AFTER_MS
}

/// Staff mark a stuck import failed: one batch, gated on its audit row
/// `roster.import.fail` (written only while the import is still `queued`
/// / `running` and older than the lock window). A Job of it that runs
/// later finds it `failed` and stops.
pub(crate) async fn mark_failed(
    s: &Services,
    id: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<Outcome> {
    let Some(imp) = store::load(s, id).await? else {
        return Ok(Outcome::Missing);
    };
    let now = s.now_ms();
    if !may_mark_failed(&imp, now) {
        return Ok(Outcome::NotPreviewed(imp.state));
    }
    let cond = Statement::new(
        "EXISTS (SELECT 1 FROM roster_import WHERE id = ? AND state IN ('queued', 'running') \
         AND COALESCE(started_at, created_at) < ?)",
    )
    .bind(id)
    .bind(now - STUCK_AFTER_MS);
    let (audit, gate) = audit::entry_if(
        s,
        actor,
        "roster.import.fail",
        Some(&Target::new("roster_import", id)),
        Some(json!({ "state": imp.state, "applied_rows": imp.applied_rows })),
        Some(json!({ "state": "failed" })),
        ctx,
        cond,
    )?;
    let failure = Failure {
        job: Some("marked failed by Staff".into()),
        ..Failure::default()
    };
    let change = Statement::new(
        "UPDATE roster_import SET state = 'failed', error = ?, finished_at = ? \
         WHERE id = ? AND state IN ('queued', 'running') \
         AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
    )
    .bind(serde_json::to_string(&failure).unwrap_or_default())
    .bind(now)
    .bind(id)
    .bind(gate);
    let done = s.db.batch(vec![audit, change]).await?;
    if done.get(1).map(|r| r.rows_affected) == Some(1) {
        Ok(Outcome::Done)
    } else {
        let state = store::load(s, id).await?.map_or(imp.state, |i| i.state);
        Ok(Outcome::NotPreviewed(state))
    }
}
