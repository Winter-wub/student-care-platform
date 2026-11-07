//! Upload and preview (#8), in the request: the file is stored under
//! `roster/<import id>/<file name>`, read, validated and compared with the
//! Students, and one `roster_import` row records the outcome: `previewed`
//! with its counts, or `failed` with the validation report. Only one
//! import may be `previewed` / `queued` / `running` at a time; a preview
//! older than an hour gives its place up (it becomes `cancelled`).

use std::collections::HashSet;

use serde_json::json;

use super::file::{self, FileError};
use super::store::{self, BUSY, Counts, Failure, Import, Preview, Tally};
use super::validate::{self, MAX_PROBLEMS, Problem, Valid};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::deployment::Deployment;
use crate::port_types::Statement;
use crate::{Error, Result, Services};

/// Largest file accepted, in bytes.
pub(crate) const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
/// A preview older than this no longer blocks a new upload.
pub(crate) const PREVIEW_HOLD_MS: i64 = 3_600_000;
/// How long the stored file is kept.
const KEEP_FILE_MS: i64 = 365 * 86_400_000;
/// Most "will become left" names listed.
const MAX_LEFT_NAMES: usize = 200;

/// What an upload did.
#[derive(Debug)]
pub(crate) enum Uploaded {
    /// Previewed: import id.
    Previewed(String),
    /// Refused with a report: import id (state `failed`).
    Refused(String),
    /// Another import holds the slot.
    Busy(Box<Import>),
}

/// `full` or `upsert`.
pub(crate) fn mode_ok(mode: &str) -> bool {
    matches!(mode, "full" | "upsert")
}

/// Stores, reads and previews `bytes`; see the module docs.
pub(crate) async fn upload(
    d: &Deployment,
    file_name: &str,
    bytes: Vec<u8>,
    mode: &str,
    staff_id: &str,
    ctx: &RequestCtx,
) -> Result<Uploaded> {
    if !mode_ok(mode) {
        return Err(Error::Invalid("roster import mode".into()));
    }
    let s = d.services();
    let now = s.now_ms();
    super::stuck::reap_busy(s).await?;
    if let Some(b) = store::busy(s, now - PREVIEW_HOLD_MS).await? {
        return Ok(Uploaded::Busy(Box::new(b)));
    }
    let id = s.new_id()?;
    let name = file::safe_name(file_name);
    let key = format!("roster/{id}/{name}");
    crate::check_file_key(&key)?;
    let sha256 = crate::auth::sha256_hex(&bytes);
    let read = if bytes.len() > MAX_FILE_BYTES {
        Err(vec![problem("too_large")])
    } else {
        let config = d.config();
        file::parse(&bytes, &name, &config.roster)
            .map_err(|e: FileError| validate::file_problems(&e))
            .and_then(|rows| validate::validate(&rows, config))
            .and_then(|v| {
                // A full file with no rows would mark everyone left.
                if mode == "full" && v.records.is_empty() {
                    Err(vec![problem("empty_full")])
                } else {
                    Ok(v)
                }
            })
    };
    let content_type = if file::is_xlsx(&name) {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    } else {
        "text/csv"
    };
    s.files.put(&key, bytes, content_type).await?;
    let counts = Counts {
        sha256,
        keep_file_until: now + KEEP_FILE_MS,
        ..Counts::default()
    };
    let row = NewRow {
        id: &id,
        staff_id,
        mode,
        key: &key,
        name: &name,
    };
    let out = record(s, &row, counts, read, mode == "full", ctx).await;
    if !matches!(out, Ok(Uploaded::Previewed(_) | Uploaded::Refused(_))) && row_absent(s, &id).await
    {
        // No row points at the objects (a failed batch, or another upload
        // took the slot): they would never be pruned. A batch can fail
        // after it committed (D1), so the row is looked up first, and the
        // objects stay unless it is surely absent.
        let _ = s.files.delete(&key).await;
        let _ = s.files.delete(&super::rows::key(&id)).await;
    }
    out
}

/// Whether import `id` surely has no row (a failed lookup says no).
async fn row_absent(s: &Services, id: &str) -> bool {
    let q = Statement::new("SELECT 1 AS x FROM roster_import WHERE id = ?").bind(id);
    s.db.query(q).await.is_ok_and(|r| r.is_empty())
}

/// Writes the import row (and, for a valid file, its parsed rows first):
/// `failed` with the report, or `previewed` while the slot is free.
async fn record(
    s: &Services,
    row: &NewRow<'_>,
    mut counts: Counts,
    read: core::result::Result<Valid, Vec<Problem>>,
    full: bool,
    ctx: &RequestCtx,
) -> Result<Uploaded> {
    let valid = match read {
        Ok(v) => v,
        Err(problems) => {
            let failure = Failure {
                total: problems.len() as u64,
                problems: problems.into_iter().take(MAX_PROBLEMS).collect(),
                job: None,
            };
            let stmts = row.statements(s, &counts, Some(&failure), 0, ctx, None)?;
            s.db.batch(stmts).await?;
            return Ok(Uploaded::Refused(row.id.to_string()));
        }
    };
    counts.preview = preview(s, &valid, full).await?;
    let parsed = super::rows::encode(&valid.records)?;
    counts.rows_sha256 = crate::auth::sha256_hex(&parsed);
    s.files
        .put(&super::rows::key(row.id), parsed, "application/jsonl")
        .await?;
    let total = valid.records.len() as i64;
    let stale = s.now_ms() - PREVIEW_HOLD_MS;
    let mut stmts = replace_stale(s, stale).await?;
    stmts.extend(row.statements(s, &counts, None, total, ctx, Some(stale))?);
    let insert_at = stmts.len() - 2;
    let done = s.db.batch(stmts).await?;
    if done.get(insert_at).map(|r| r.rows_affected) != Some(1) {
        // Another upload took the slot between the check and the write.
        return match store::busy(s, stale).await? {
            Some(b) => Ok(Uploaded::Busy(Box::new(b))),
            None => Err(Error::Conflict("roster import slot".into())),
        };
    }
    Ok(Uploaded::Previewed(row.id.to_string()))
}

/// Cancels the previews older than `stale` (they give the slot up), each
/// with an audit row `roster.import.cancel` by the system that gates it.
async fn replace_stale(s: &Services, stale: i64) -> Result<Vec<Statement>> {
    let rows =
        s.db.query(
            Statement::new(
                "SELECT id FROM roster_import WHERE state = 'previewed' AND created_at < ?",
            )
            .bind(stale),
        )
        .await?;
    let mut stmts = Vec::new();
    for i in 0..rows.len() {
        let Some(id) = rows.get(i, "id").and_then(crate::Value::as_text) else {
            continue;
        };
        let cond = Statement::new(
            "EXISTS (SELECT 1 FROM roster_import WHERE id = ? AND state = 'previewed' \
             AND created_at < ?)",
        )
        .bind(id)
        .bind(stale);
        let (audit, gate) = audit::entry_if(
            s,
            &Actor::System,
            "roster.import.cancel",
            Some(&Target::new("roster_import", id)),
            Some(json!({ "state": "previewed" })),
            Some(json!({ "state": "cancelled", "reason": "replaced after an hour" })),
            &RequestCtx::default(),
            cond,
        )?;
        stmts.push(audit);
        stmts.push(
            Statement::new(
                "UPDATE roster_import SET state = 'cancelled', finished_at = ? WHERE id = ? \
                 AND state = 'previewed' AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
            )
            .bind(s.now_ms())
            .bind(id)
            .bind(gate),
        );
    }
    Ok(stmts)
}

/// The new `roster_import` row.
struct NewRow<'a> {
    id: &'a str,
    staff_id: &'a str,
    mode: &'a str,
    key: &'a str,
    name: &'a str,
}

impl NewRow<'_> {
    /// The insert (only while the slot is free, when `slot` holds the
    /// stale limit) and its audit row `roster.import.upload`.
    fn statements(
        &self,
        s: &Services,
        counts: &Counts,
        failure: Option<&Failure>,
        total: i64,
        ctx: &RequestCtx,
        slot: Option<i64>,
    ) -> Result<Vec<Statement>> {
        let state = if failure.is_some() {
            "failed"
        } else {
            "previewed"
        };
        let now = s.now_ms();
        let free = match slot {
            Some(_) => format!("WHERE NOT EXISTS (SELECT 1 FROM roster_import r WHERE {BUSY})"),
            None => String::new(),
        };
        let insert = Statement::new(format!(
            "INSERT INTO roster_import (id, staff_id, mode, state, file_key, file_name, \
             total_rows, counts, error, created_at, finished_at) \
             SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ? {free}"
        ))
        .bind(self.id)
        .bind(self.staff_id)
        .bind(self.mode)
        .bind(state)
        .bind(self.key)
        .bind(self.name)
        .bind(total)
        .bind(serde_json::to_string(counts).map_err(json_err)?)
        .bind(
            failure
                .map(serde_json::to_string)
                .transpose()
                .map_err(json_err)?,
        )
        .bind(now)
        .bind(failure.map(|_| now));
        let insert = match slot {
            Some(stale) => insert.bind(stale),
            None => insert,
        };
        let after = json!({
            "file_name": self.name,
            "mode": self.mode,
            "state": state,
            "sha256": counts.sha256,
            "total_rows": total,
            "problems": failure.map(|f| f.total),
        });
        let (audit, _) = audit::entry_if(
            s,
            &Actor::Staff(self.staff_id.into()),
            "roster.import.upload",
            Some(&Target::new("roster_import", self.id)),
            None,
            Some(after),
            ctx,
            Statement::new("EXISTS (SELECT 1 FROM roster_import WHERE id = ?)").bind(self.id),
        )?;
        Ok(vec![insert, audit])
    }
}

/// A problem of the file as a whole.
fn problem(reason: &str) -> Problem {
    Problem {
        row: 0,
        column: String::new(),
        reason: reason.into(),
    }
}

fn json_err(e: serde_json::Error) -> Error {
    Error::Invalid(format!("roster counts: {e}"))
}

/// Compares the file with the Students.
async fn preview(s: &Services, valid: &Valid, full: bool) -> Result<Preview> {
    let existing = store::all_students(s).await?;
    let mut tally = Tally::default();
    let mut listed = HashSet::new();
    for rec in &valid.records {
        listed.insert(rec.code.as_str());
        tally.add(&store::classify(existing.get(&rec.code), rec).0);
    }
    let active: Vec<_> = existing
        .iter()
        .filter(|(_, e)| e.fields.status == "active")
        .collect();
    let mut leaving: Vec<&str> = Vec::new();
    let mut kept_by_status = 0;
    if full {
        for (code, e) in &active {
            if listed.contains(code.as_str()) {
                continue;
            }
            if store::status_held(e) {
                kept_by_status += 1;
            } else {
                leaving.push(&e.fields.display_name);
            }
        }
    }
    leaving.sort_unstable();
    let left = leaving.len() as u64;
    let active_before = active.len() as u64;
    let shrinking = full && (valid.records.len() as u64) < active_before;
    Ok(Preview {
        tally,
        will_become_left: left,
        kept_by_status,
        empty_email: valid.empty_email as u64,
        active_before,
        warning: left * 5 > active_before || shrinking,
        left_more: left.saturating_sub(MAX_LEFT_NAMES as u64),
        left_names: leaving
            .into_iter()
            .take(MAX_LEFT_NAMES)
            .map(str::to_string)
            .collect(),
    })
}
