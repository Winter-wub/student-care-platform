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
    if let Some(b) = store::busy(s, now - PREVIEW_HOLD_MS).await? {
        return Ok(Uploaded::Busy(Box::new(b)));
    }
    let id = s.new_id()?;
    let name = safe_name(file_name);
    let key = format!("roster/{id}/{name}");
    crate::check_file_key(&key)?;
    let sha256 = crate::auth::sha256_hex(&bytes);
    let read = if bytes.len() > MAX_FILE_BYTES {
        Err(vec![Problem {
            row: 0,
            column: String::new(),
            reason: "too_large".into(),
        }])
    } else {
        let config = d.config();
        file::parse(&bytes, &name, &config.roster)
            .map_err(|e: FileError| validate::file_problems(&e))
            .and_then(|rows| validate::validate(&rows, config))
    };
    let content_type = if file::is_xlsx(&bytes, &name) {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    } else {
        "text/csv"
    };
    s.files.put(&key, bytes, content_type).await?;
    let mut counts = Counts {
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
            return Ok(Uploaded::Refused(id));
        }
    };
    counts.preview = preview(s, &valid, mode == "full").await?;
    let total = valid.records.len() as i64;
    let stale = now - PREVIEW_HOLD_MS;
    let mut stmts = vec![
        Statement::new(
            "UPDATE roster_import SET state = 'cancelled', finished_at = ? \
             WHERE state = 'previewed' AND created_at < ?",
        )
        .bind(now)
        .bind(stale),
    ];
    stmts.extend(row.statements(s, &counts, None, total, ctx, Some(stale))?);
    let done = s.db.batch(stmts).await?;
    if done.get(1).map(|r| r.rows_affected) != Some(1) {
        // Another upload took the slot between the check and the write.
        let _ = s.files.delete(&key).await;
        return match store::busy(s, stale).await? {
            Some(b) => Ok(Uploaded::Busy(Box::new(b))),
            None => Err(Error::Conflict("roster import slot".into())),
        };
    }
    Ok(Uploaded::Previewed(id))
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
    if full {
        for (code, e) in &active {
            if listed.contains(code.as_str()) {
                continue;
            }
            if store::status_held(e) {
                tally.skipped_by_override += 1;
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

/// The base name of an uploaded file, safe as a FileStore key segment:
/// no directories, no `\ : /` or control characters, at most 100
/// characters; `roster` when nothing is left.
pub(crate) fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, ':' | '"') {
                '_'
            } else {
                c
            }
        })
        .take(100)
        .collect();
    let clean = clean.trim().trim_start_matches('.').to_string();
    if clean.is_empty() {
        "roster".into()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::safe_name;

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(
            safe_name("C:\\Users\\x\\รายชื่อ 2569.xlsx"),
            "รายชื่อ 2569.xlsx"
        );
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name(".."), "roster");
        assert_eq!(safe_name("a:b\n.csv"), "a_b_.csv");
        assert_eq!(safe_name(""), "roster");
        for n in ["x", "..", "a/b", "ok.csv"] {
            assert!(crate::check_file_key(&format!("roster/id/{}", safe_name(n))).is_ok());
        }
    }
}
