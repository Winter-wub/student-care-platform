//! Upload and preview (#8), in the request: the file is stored under
//! `roster/<import id>/<file name>`, read, validated and compared with the
//! Students, and one `roster_import` row records the outcome: `previewed`
//! with its counts, or `failed` with the validation report. Only one
//! import may be `previewed` / `queued` / `running` at a time; a preview
//! older than an hour gives its place up (it becomes `cancelled`).

use std::collections::{HashMap, HashSet};

use serde_json::json;

use super::diff::{self, Change};
use super::file::{self, FoundHeaders};
use super::intake::{self, Checked};
use super::store::{self, BUSY, Counts, Existing, Failure, Import, Preview, Tally, Warned};
use super::validate::{Problem, Valid};
use super::warnings;
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

/// What an upload did.
#[derive(Debug)]
pub(crate) enum Uploaded {
    /// Previewed: import id, and whether the preview carries the leave
    /// warning (more than 20 % left, or a file shorter than the roster).
    Previewed(String, bool),
    /// Refused with a report: import id (state `failed`).
    Refused(String),
    /// Another import holds the slot.
    Busy(Box<Import>),
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
    // The Deployment's `[roster] modes`, for every caller (page and code).
    let config = d.config();
    if !config.roster.allows_mode(mode) {
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
    let example = intake::Example::of(d.catalog(), config);
    let read = intake::read(s, config, &name, &bytes, mode, &example).await?;
    let content_type = if file::is_xlsx(&name) {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    } else {
        "text/csv"
    };
    s.files.put(&key, bytes, content_type).await?;
    let counts = Counts {
        sha256,
        skipped: read.skipped,
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
    let input = Input {
        read: read.result,
        headers: read.headers,
        full: mode == "full",
    };
    let out = record(s, &row, counts, input, ctx).await;
    if !matches!(out, Ok(Uploaded::Previewed(..) | Uploaded::Refused(_)))
        && row_absent(s, &id).await
    {
        // No row points at the objects (a failed batch, or another upload
        // took the slot): they would never be pruned. A batch can fail
        // after it committed (D1), so the row is looked up first, and the
        // objects stay unless it is surely absent.
        let _ = s.files.delete(&key).await;
        let _ = s.files.delete(&super::rows::key(&id)).await;
        let _ = diff::delete(s, &id).await;
    }
    out
}

/// Whether import `id` surely has no row (a failed lookup says no).
async fn row_absent(s: &Services, id: &str) -> bool {
    let q = Statement::new("SELECT 1 AS x FROM roster_import WHERE id = ?").bind(id);
    s.db.query(q).await.is_ok_and(|r| r.is_empty())
}

/// What reading the upload found, for [`record`].
struct Input {
    read: core::result::Result<Checked, Vec<Problem>>,
    /// The header rows of a file refused for its headers (#134).
    headers: Vec<FoundHeaders>,
    full: bool,
}

/// Writes the import row (and, for a valid file, its parsed rows and its
/// changes first): `failed` with the report, or `previewed` while the
/// slot is free.
async fn record(
    s: &Services,
    row: &NewRow<'_>,
    mut counts: Counts,
    input: Input,
    ctx: &RequestCtx,
) -> Result<Uploaded> {
    let full = input.full;
    let Checked { valid, existing } = match input.read {
        Ok(c) => c,
        Err(problems) => {
            let (blocking, warned): (Vec<_>, Vec<_>) =
                problems.into_iter().partition(|p| p.severity.is_blocking());
            counts.warnings = Warned::new(warned);
            let failure = Failure {
                total: warnings::total(&blocking),
                classes: warnings::classes(&blocking),
                problems: warnings::kept(blocking),
                job: None,
                headers: input.headers,
            };
            let stmts = row.statements(s, &counts, Some(&failure), 0, ctx, None)?;
            s.db.batch(stmts).await?;
            return Ok(Uploaded::Refused(row.id.to_string()));
        }
    };
    // Live Links matter only to those who will become left (#134).
    let links = if full {
        diff::live_links(s).await?
    } else {
        HashMap::new()
    };
    let (preview, changes) = preview(&valid, &existing, full, &links);
    counts.preview = preview;
    diff::store(s, row.id, &changes).await?;
    counts.warnings = Warned::new(valid.warnings.clone());
    let parsed = super::rows::encode(&valid.records)?;
    counts.rows_sha256 = crate::auth::sha256_hex(&parsed);
    s.files
        .put(&super::rows::key(row.id), parsed, "application/jsonl")
        .await?;
    let total = valid.records.len() as i64;
    let stale = s.now_ms() - PREVIEW_HOLD_MS;
    let (mut stmts, replaced) = replace_stale(s, stale).await?;
    stmts.extend(row.statements(s, &counts, None, total, ctx, Some(stale))?);
    let insert_at = stmts.len() - 2;
    let done = s.db.batch(stmts).await?;
    // Nothing reads a cancelled import's lists (#134): best effort.
    for (k, id) in replaced.iter().enumerate() {
        if done.get(2 * k + 1).map(|r| r.rows_affected) == Some(1) {
            let _ = diff::delete(s, id).await;
        }
    }
    if done.get(insert_at).map(|r| r.rows_affected) != Some(1) {
        // Another upload took the slot between the check and the write.
        return match store::busy(s, stale).await? {
            Some(b) => Ok(Uploaded::Busy(Box::new(b))),
            None => Err(Error::Conflict("roster import slot".into())),
        };
    }
    Ok(Uploaded::Previewed(
        row.id.to_string(),
        counts.preview.warning,
    ))
}

/// Cancels the previews older than `stale` (they give the slot up), each
/// with an audit row `roster.import.cancel` by the system that gates it:
/// the statements (an audit row, then its update, per import) and the ids.
async fn replace_stale(s: &Services, stale: i64) -> Result<(Vec<Statement>, Vec<String>)> {
    let rows =
        s.db.query(
            Statement::new(
                "SELECT id FROM roster_import WHERE state = 'previewed' AND created_at < ?",
            )
            .bind(stale),
        )
        .await?;
    let mut stmts = Vec::new();
    let mut ids = Vec::new();
    for i in 0..rows.len() {
        let Some(id) = rows.get(i, "id").and_then(crate::Value::as_text) else {
            continue;
        };
        ids.push(id.to_string());
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
    Ok((stmts, ids))
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

fn json_err(e: serde_json::Error) -> Error {
    Error::Invalid(format!("roster counts: {e}"))
}

/// Compares the file with the Students: the preview, and every change
/// Student by Student (#134; `links`: live Links per Student id).
pub(super) fn preview(
    valid: &Valid,
    existing: &HashMap<String, Existing>,
    full: bool,
    links: &HashMap<String, u64>,
) -> (Preview, Vec<Change>) {
    let mut tally = Tally::default();
    let mut listed = HashSet::new();
    let mut changes = Vec::new();
    for rec in &valid.records {
        listed.insert(rec.code.as_str());
        let e = existing.get(&rec.code);
        let (t, m) = store::classify(e, rec);
        tally.add(&t);
        changes.extend(diff::change(e, rec, &t, &m));
    }
    let active: Vec<_> = existing
        .iter()
        .filter(|(_, e)| e.fields.status == "active")
        .collect();
    let mut leavers = Vec::new();
    let mut kept_by_status = 0;
    if full {
        for (code, e) in &active {
            if listed.contains(code.as_str()) {
                continue;
            }
            if store::status_held(e) {
                kept_by_status += 1;
            } else {
                leavers.push(diff::leaving(code, e, links));
            }
        }
    }
    leavers.sort_unstable_by(|a, b| a.code.cmp(&b.code));
    let left = leavers.len() as u64;
    let active_before = active.len() as u64;
    let shrinking = full && (valid.records.len() as u64) < active_before;
    let preview = Preview {
        tally,
        will_become_left: left,
        kept_by_status,
        empty_email: valid.empty_email as u64,
        active_before,
        warning: left * 5 > active_before || shrinking,
        // The leavers are in the stored head (#134), not in `counts`.
        left_more: 0,
        left_names: Vec::new(),
        detailed: true,
        links_cut: leavers.iter().map(|c| c.links).sum(),
    };
    changes.extend(leavers);
    (preview, changes)
}
