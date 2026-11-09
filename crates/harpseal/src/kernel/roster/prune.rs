//! Deleting stored roster files a year after upload (#46). Each tick
//! takes up to [`PER_TICK`] imports past `counts.keep_file_until` that
//! still have a file and are not `queued` / `running` (a `previewed` one
//! that old was abandoned; confirming it now fails its Job),
//! deletes the original, its parsed rows (`rows.rs`) and its list of
//! changes (`diff.rs`, #134) from the
//! FileStore, then, in one batch, writes the audit row `roster.file.prune`
//! (actor `system`) and clears `file_key` to `''` (the column is
//! `NOT NULL`; a backup skips empty keys), and drops the Students the row
//! names (#134: who left, the Student of each problem). The objects go
//! first: a failed
//! batch is retried on the next tick, and deleting a missing object is
//! not an error. The import row, its counts and its report stay.

use serde_json::json;

use super::store::{Counts, Failure};
use super::validate::Problem;
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::port_types::{Statement, Value};
use crate::{Result, Services};

/// Imports whose files one tick deletes, at most.
const PER_TICK: i64 = 20;

/// What one prune did: imports that lost their files, and the imports it
/// could not prune (each is retried on the next tick; one failing object
/// never holds the others back).
#[derive(Debug, Default)]
pub(crate) struct Pruned {
    pub(crate) count: u64,
    pub(crate) errors: Vec<String>,
}

/// Deletes the files due at `now`.
pub(crate) async fn prune_files(s: &Services, now: i64) -> Result<Pruned> {
    let due = Statement::new(
        "SELECT id, file_key, file_name, counts, error FROM roster_import \
         WHERE file_key <> '' AND state NOT IN ('queued', 'running') \
         AND json_extract(counts, '$.keep_file_until') < ? \
         ORDER BY created_at, id LIMIT ?",
    )
    .bind(now)
    .bind(PER_TICK);
    let rows = s.db.query(due).await?;
    let mut out = Pruned::default();
    for i in 0..rows.len() {
        let text = |c: &str| rows.get(i, c).and_then(Value::as_text).unwrap_or_default();
        let (id, key, name) = (text("id"), text("file_key"), text("file_name"));
        let kept = scrub(
            text("counts"),
            rows.get(i, "error").and_then(Value::as_text),
        );
        match prune_one(s, (id, key, name), kept).await {
            Ok(n) => out.count += n,
            Err(e) => out.errors.push(format!("roster import {id}: {e}")),
        }
    }
    Ok(out)
}

/// `counts` and `error` without the Students they name (#134): the names
/// of those who left (`left_names`, stored before #134) and the Student
/// a problem names. The counts, rows and reasons stay.
fn scrub(counts: &str, error: Option<&str>) -> (String, Option<String>) {
    let anonymous = |p: &mut Problem| {
        p.student.clear();
        p.name.clear();
    };
    // Counts that no longer read stay as they are, never reset.
    let counts = match serde_json::from_str::<Counts>(counts) {
        Ok(mut c) => {
            c.preview.left_names.clear();
            c.warnings.problems.iter_mut().for_each(anonymous);
            serde_json::to_string(&c).unwrap_or_else(|_| counts.to_string())
        }
        Err(_) => counts.to_string(),
    };
    let error = error.map(|e| match serde_json::from_str::<Failure>(e) {
        Ok(mut f) => {
            f.problems.iter_mut().for_each(anonymous);
            serde_json::to_string(&f).unwrap_or_else(|_| e.to_string())
        }
        Err(_) => e.to_string(),
    });
    (counts, error)
}

/// Deletes one import's objects, then audits, clears `file_key` and
/// writes the scrubbed `kept` counts and error.
async fn prune_one(
    s: &Services,
    row: (&str, &str, &str),
    kept: (String, Option<String>),
) -> Result<u64> {
    let (id, key, name) = row;
    let rows_key = super::rows::key(id);
    let (diff_key, head_key) = (super::diff::key(id), super::diff::head_key(id));
    s.files.delete(key).await?;
    s.files.delete(&rows_key).await?;
    super::diff::delete(s, id).await?;
    let cond = Statement::new("EXISTS (SELECT 1 FROM roster_import WHERE id = ? AND file_key = ?)")
        .bind(id)
        .bind(key);
    // After `file_key = ''` this row is the only record of what was stored.
    let (audit, gate) = audit::entry_if(
        s,
        &Actor::System,
        "roster.file.prune",
        Some(&Target::new("roster_import", id)),
        Some(json!({ "file_name": name, "stored": true })),
        Some(
            json!({ "stored": false, "keys": [key, rows_key, diff_key, head_key], "reason": "kept for one year" }),
        ),
        &RequestCtx::default(),
        cond,
    )?;
    let clear = Statement::new(
        "UPDATE roster_import SET file_key = '', counts = ?, error = ? WHERE id = ? \
         AND file_key = ? AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
    )
    .bind(kept.0)
    .bind(kept.1)
    .bind(id)
    .bind(key)
    .bind(gate);
    let done = s.db.batch(vec![audit, clear]).await?;
    Ok(done.get(1).map_or(0, |r| r.rows_affected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pruned_row_keeps_its_counts_and_problems_but_names_no_student() {
        let mut c = Counts::default();
        c.preview.left_names = vec!["นก ทะเล".into()];
        c.preview.will_become_left = 1;
        c.warnings = super::super::store::Warned::new(vec![
            Problem::new(2, "อีเมล", "missing_email").about("68000001", "นก ทะเล"),
        ]);
        let f = Failure {
            problems: vec![Problem::new(3, "อีเมล", "invalid_email").about("68000002", "ปลา")],
            total: 1,
            ..Failure::default()
        };
        let (counts, error) = scrub(
            &serde_json::to_string(&c).unwrap(),
            Some(&serde_json::to_string(&f).unwrap()),
        );
        for kept in [&counts, error.as_ref().unwrap()] {
            assert!(
                !kept.contains("6800000") && !kept.contains("นก") && !kept.contains("ปลา"),
                "{kept}"
            );
        }
        let c: Counts = serde_json::from_str(&counts).unwrap();
        assert_eq!(
            (
                c.preview.will_become_left,
                c.warnings.total,
                c.warnings.problems[0].row
            ),
            (1, 1, 2)
        );
        let f: Failure = serde_json::from_str(error.as_deref().unwrap()).unwrap();
        assert_eq!(
            (f.total, f.problems[0].reason.as_str()),
            (1, "invalid_email")
        );
        assert_eq!(scrub("not json", None), ("not json".to_string(), None));
    }
}
