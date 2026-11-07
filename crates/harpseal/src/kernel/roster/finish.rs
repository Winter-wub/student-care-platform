//! The left phase of a `full` import, one batch per Job run (payload
//! `{import_id, chunk, left: n}`, key `roster:<id>:left:<n>`): up to 100
//! active Students the file did not list (`roster_imported_at` before the
//! import's `started_at`, status not set by hand) become `left`.
//!
//! The batch claims its turn with a compare-and-set on
//! `counts.left_batches` (n → n + 1) and a per-run token in `counts.run`;
//! every other statement is gated on both, so a repeated or concurrent
//! run writes nothing. Per Student, a `student.status` audit row by the
//! Job gates the status change and the revoke of each live Link (its own
//! `link.revoke` row). The same batch adds the rows it wrote to `left` /
//! `links_revoked` and queues batch n + 1, or, when fewer than 100 were
//! left to do, sets the import `done`. The revoked notices are Pushed after
//! the commit; a failed one is counted, never an error.

use serde_json::json;

use super::service::{JOB_KIND, left_key};
use super::store::Import;
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::manifest::Job;
use crate::module::JobContext;
use crate::port_types::{Statement, Value};
use crate::{Error, Result, links};

/// Students marked left per Job run.
pub(crate) const LEFT_BATCH: usize = 100;

/// Students the import will mark left, as an SQL condition with one
/// parameter (`started_at`).
const NOT_LISTED: &str = "status = 'active' \
    AND (roster_imported_at IS NULL OR roster_imported_at < ?) \
    AND NOT EXISTS (SELECT 1 FROM json_each(manual_overrides) WHERE value = 'status')";

/// Students the import keeps active because Staff set their status by
/// hand (one parameter: `started_at`); the preview's `kept_by_status`.
const KEPT: &str = "status = 'active' \
    AND (roster_imported_at IS NULL OR roster_imported_at < ?) \
    AND EXISTS (SELECT 1 FROM json_each(manual_overrides) WHERE value = 'status')";

/// The gate: this run's compare-and-set went through.
const GATE: &str = "EXISTS (SELECT 1 FROM roster_import WHERE id = ? \
    AND json_extract(counts, '$.left_batches') = ? AND json_extract(counts, '$.run') = ?)";

/// Runs left batch `n` of `imp`.
pub(super) async fn left_batch(cx: &JobContext, job: &Job, imp: &Import, n: i64) -> Result<()> {
    let s = &cx.services;
    if imp.state != "running" || imp.applied_rows < imp.total_rows {
        return Ok(());
    }
    let done = imp.counts.left_batches;
    if done > n {
        // Batch n is applied; make sure the next one is queued.
        let payload = json!({ "import_id": imp.id, "chunk": imp.total_rows, "left": done });
        cx.jobs
            .enqueue_unique(&left_key(&imp.id, done), JOB_KIND, payload, None)
            .await?;
        return Ok(());
    }
    if done < n {
        return Err(Error::Invalid(
            "roster left batch ahead of the applied ones".into(),
        ));
    }
    let started = imp.started_at.unwrap_or_else(|| s.now_ms());
    let pick = Statement::new(format!(
        "SELECT id FROM student WHERE {NOT_LISTED} ORDER BY id LIMIT ?"
    ))
    .bind(started)
    .bind(LEFT_BATCH as i64);
    let rows = s.db.query(pick).await?;
    let ids: Vec<String> = (0..rows.len())
        .filter_map(|i| {
            rows.get(i, "id")
                .and_then(Value::as_text)
                .map(str::to_string)
        })
        .collect();
    let mut live = links::live_links_of_students(s, &ids, &cx.config.default_locale).await?;

    let token = s.random_hex(8)?;
    let gate = |st: Statement| st.bind(imp.id.as_str()).bind(n + 1).bind(token.as_str());
    let actor = Actor::Job(job.id.clone());
    let ctx = RequestCtx::default();
    let batch_start = s.now_ms();
    let mut stmts = vec![
        Statement::new(
            "UPDATE roster_import SET counts = json_set(counts, '$.left_batches', ?, '$.run', ?) \
             WHERE id = ? AND state = 'running' AND applied_rows = total_rows \
             AND COALESCE(json_extract(counts, '$.left_batches'), 0) = ?",
        )
        .bind(n + 1)
        .bind(token.as_str())
        .bind(imp.id.as_str())
        .bind(n),
    ];
    // Indexes of the Link updates, with the Link, to notify after commit.
    let mut revokes = Vec::new();
    for id in &ids {
        let cond = gate(Statement::new(format!(
            "{GATE} AND EXISTS (SELECT 1 FROM student WHERE id = ? AND {NOT_LISTED})"
        )))
        .bind(id.as_str())
        .bind(started);
        let (audit_row, row_id) = audit::entry_if(
            s,
            &actor,
            "student.status",
            Some(&Target::new("student", id.as_str())),
            Some(json!({ "status": "active" })),
            Some(json!({ "status": "left", "roster_import": imp.id })),
            &ctx,
            cond,
        )?;
        stmts.push(audit_row);
        stmts.push(
            Statement::new(
                "UPDATE student SET status = 'left', updated_at = ? WHERE id = ? \
                 AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
            )
            .bind(s.now_ms())
            .bind(id.as_str())
            .bind(row_id.as_str()),
        );
        let mine: Vec<_> = live.extract_if(.., |(sid, _)| sid == id).collect();
        for (_, link) in mine {
            let more = links::staff_revoke_statements(s, &link, &actor, &ctx, Some(&row_id))?;
            revokes.push((stmts.len() + 1, link));
            stmts.extend(more);
        }
    }
    let written = |action: &str| {
        format!(
            "(SELECT count(*) FROM audit_log WHERE at >= {batch_start} AND action = '{action}' \
             AND actor_kind = 'job' AND actor_id = ?)"
        )
    };
    stmts.push(gate(
        Statement::new(format!(
            "UPDATE roster_import SET counts = json_set(counts, \
             '$.left', COALESCE(json_extract(counts, '$.left'), 0) + {}, \
             '$.links_revoked', COALESCE(json_extract(counts, '$.links_revoked'), 0) + {}) \
             WHERE id = ? AND {GATE}",
            written("student.status"),
            written("link.revoke")
        ))
        .bind(job.id.as_str())
        .bind(job.id.as_str())
        .bind(imp.id.as_str()),
    ));
    if ids.len() == LEFT_BATCH {
        let payload = json!({ "import_id": imp.id, "chunk": imp.total_rows, "left": n + 1 });
        let key = left_key(&imp.id, n + 1);
        stmts.push(cx.jobs.insert_unique(&key, JOB_KIND, &payload, None)?.1);
    } else {
        stmts.push(gate(
            Statement::new(format!(
                "UPDATE roster_import SET state = 'done', finished_at = ?, \
                 counts = json_set(counts, '$.kept_by_status', \
                 (SELECT count(*) FROM student WHERE {KEPT})) \
                 WHERE id = ? AND state = 'running' AND {GATE}"
            ))
            .bind(s.now_ms())
            .bind(started)
            .bind(imp.id.as_str()),
        ));
    }
    let results = s.db.batch(stmts).await?;
    if results.first().map(|r| r.rows_affected) != Some(1) {
        return Ok(());
    }
    let mut failed = 0i64;
    for (i, link) in revokes {
        if results.get(i).map_or(0, |r| r.rows_affected) == 0 {
            continue;
        }
        if !matches!(links::notify_revoked(&cx.deployment, link).await, Ok(true)) {
            failed += 1;
        }
    }
    if failed > 0 {
        let add = Statement::new(
            "UPDATE roster_import SET counts = json_set(counts, '$.notices_failed', \
             COALESCE(json_extract(counts, '$.notices_failed'), 0) + ?) WHERE id = ?",
        )
        .bind(failed)
        .bind(imp.id.as_str());
        s.db.execute(add).await?;
    }
    Ok(())
}
