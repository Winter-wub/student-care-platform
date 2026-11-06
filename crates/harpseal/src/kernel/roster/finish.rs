//! After the last chunk: in `full` mode, every active Student the file did
//! not list (`roster_imported_at` before the import's `started_at`)
//! becomes `left`, 100 per batch, unless Staff set the status by hand (an
//! override). Each one gets a `student.status` audit row by the Job, which
//! gates the change and the revoke of every live Link (each with its own
//! `link.revoke` row). After each batch the revoked notice is Pushed per
//! Link; a notice that fails is counted, never an error. Then the import
//! is `done`. A re-run after a crash continues where it stopped.

use serde_json::json;

use super::store::Import;
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::manifest::Job;
use crate::module::JobContext;
use crate::port_types::{Statement, Value};
use crate::{Result, Services, links};

/// Students marked left per batch.
const LEFT_BATCH: i64 = 100;

/// Students the import will mark left, as an SQL condition with one
/// parameter (`started_at`).
const NOT_LISTED: &str = "status = 'active' \
    AND (roster_imported_at IS NULL OR roster_imported_at < ?) \
    AND NOT EXISTS (SELECT 1 FROM json_each(manual_overrides) WHERE value = 'status')";

pub(super) async fn finish(cx: &JobContext, job: &Job, imp: &Import) -> Result<()> {
    let s = &cx.services;
    if imp.mode == "full" {
        let started = imp.started_at.unwrap_or_else(|| s.now_ms());
        while leave_batch(cx, job, imp, started).await? > 0 {}
    }
    let done = Statement::new(
        "UPDATE roster_import SET state = 'done', finished_at = ? WHERE id = ? AND state = 'running'",
    )
    .bind(s.now_ms())
    .bind(imp.id.as_str());
    s.db.execute(done).await?;
    Ok(())
}

/// Marks up to 100 Students left; returns how many changed.
async fn leave_batch(cx: &JobContext, job: &Job, imp: &Import, started: i64) -> Result<u64> {
    let s = &cx.services;
    let pick = Statement::new(format!(
        "SELECT id FROM student WHERE {NOT_LISTED} ORDER BY id LIMIT ?"
    ))
    .bind(started)
    .bind(LEFT_BATCH);
    let rows = s.db.query(pick).await?;
    let ids: Vec<String> = (0..rows.len())
        .filter_map(|i| {
            rows.get(i, "id")
                .and_then(Value::as_text)
                .map(str::to_string)
        })
        .collect();
    if ids.is_empty() {
        return Ok(0);
    }
    let actor = Actor::Job(job.id.clone());
    let ctx = RequestCtx::default();
    let default = &cx.config.default_locale;
    let mut stmts = Vec::new();
    // (index of the student update, Links with the index of their update)
    let mut plan = Vec::new();
    for id in &ids {
        let (audit_row, gate) = audit::entry_if(
            s,
            &actor,
            "student.status",
            Some(&Target::new("student", id.as_str())),
            Some(json!({ "status": "active" })),
            Some(json!({ "status": "left", "roster_import": imp.id })),
            &ctx,
            Statement::new(format!(
                "EXISTS (SELECT 1 FROM student WHERE id = ? AND {NOT_LISTED})"
            ))
            .bind(id.as_str())
            .bind(started),
        )?;
        stmts.push(audit_row);
        stmts.push(
            Statement::new(
                "UPDATE student SET status = 'left', updated_at = ? WHERE id = ? \
                 AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
            )
            .bind(s.now_ms())
            .bind(id.as_str())
            .bind(gate.as_str()),
        );
        let at = stmts.len() - 1;
        let mut revokes = Vec::new();
        for link in links::live_links_of(s, id, default).await? {
            let more = links::staff_revoke_statements(s, &link, &actor, &ctx, Some(&gate))?;
            revokes.push((stmts.len() + 1, link));
            stmts.extend(more);
        }
        plan.push((at, revokes));
    }
    let done = s.db.batch(stmts).await?;
    let hit = |i: usize| done.get(i).map_or(0, |r| r.rows_affected);
    let (mut left, mut revoked, mut failed) = (0u64, 0u64, 0u64);
    for (at, revokes) in plan {
        left += hit(at);
        for (i, link) in revokes {
            if hit(i) == 0 {
                continue;
            }
            revoked += 1;
            if !matches!(links::notify_revoked(&cx.deployment, link).await, Ok(true)) {
                failed += 1;
            }
        }
    }
    add_counts(s, &imp.id, left, revoked, failed).await?;
    Ok(left)
}

async fn add_counts(s: &Services, id: &str, left: u64, revoked: u64, failed: u64) -> Result<()> {
    let n = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
    let stmt = Statement::new(
        "UPDATE roster_import SET counts = json_set(counts, \
         '$.left', COALESCE(json_extract(counts, '$.left'), 0) + ?, \
         '$.links_revoked', COALESCE(json_extract(counts, '$.links_revoked'), 0) + ?, \
         '$.notices_failed', COALESCE(json_extract(counts, '$.notices_failed'), 0) + ?) \
         WHERE id = ?",
    )
    .bind(n(left))
    .bind(n(revoked))
    .bind(n(failed))
    .bind(id);
    s.db.execute(stmt).await.map(|_| ())
}
