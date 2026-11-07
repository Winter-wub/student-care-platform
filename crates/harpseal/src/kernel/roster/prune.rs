//! Deleting stored roster files a year after upload (#46). Each tick
//! takes up to [`PER_TICK`] imports past `counts.keep_file_until` that
//! still have a file and are not `queued` / `running` (a `previewed` one
//! that old was abandoned; confirming it now fails its Job),
//! deletes the original and its parsed rows (`rows.rs`) from the
//! FileStore, then, in one batch, writes the audit row `roster.file.prune`
//! (actor `system`) and clears `file_key` to `''` (the column is
//! `NOT NULL`; a backup skips empty keys). The objects go first: a failed
//! batch is retried on the next tick, and deleting a missing object is
//! not an error. The import row, its counts and its report stay.

use serde_json::json;

use crate::audit::{self, Actor, RequestCtx, Target};
use crate::port_types::{Statement, Value};
use crate::{Result, Services};

/// Imports whose files one tick deletes, at most.
const PER_TICK: i64 = 20;

/// Deletes the files due at `now`; returns how many imports lost theirs.
pub(crate) async fn prune_files(s: &Services, now: i64) -> Result<u64> {
    let due = Statement::new(
        "SELECT id, file_key, file_name FROM roster_import \
         WHERE file_key <> '' AND state NOT IN ('queued', 'running') \
         AND json_extract(counts, '$.keep_file_until') < ? \
         ORDER BY created_at, id LIMIT ?",
    )
    .bind(now)
    .bind(PER_TICK);
    let rows = s.db.query(due).await?;
    let mut pruned = 0;
    for i in 0..rows.len() {
        let text = |c: &str| rows.get(i, c).and_then(Value::as_text).unwrap_or_default();
        let (id, key) = (text("id"), text("file_key"));
        s.files.delete(key).await?;
        s.files.delete(&super::rows::key(id)).await?;
        let cond =
            Statement::new("EXISTS (SELECT 1 FROM roster_import WHERE id = ? AND file_key = ?)")
                .bind(id)
                .bind(key);
        let (audit, gate) = audit::entry_if(
            s,
            &Actor::System,
            "roster.file.prune",
            Some(&Target::new("roster_import", id)),
            Some(json!({ "file_name": text("file_name"), "stored": true })),
            Some(json!({ "stored": false, "reason": "kept for one year" })),
            &RequestCtx::default(),
            cond,
        )?;
        let clear = Statement::new(
            "UPDATE roster_import SET file_key = '' WHERE id = ? AND file_key = ? \
             AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
        )
        .bind(id)
        .bind(key)
        .bind(gate);
        let done = s.db.batch(vec![audit, clear]).await?;
        pruned += done.get(1).map_or(0, |r| r.rows_affected);
    }
    Ok(pruned)
}
