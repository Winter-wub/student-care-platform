//! The compare-and-set every Staff write goes through: an audit row
//! written only while the announcement is still at the status and version
//! the page showed, then the changes gated on that row, in one batch.

use serde_json::{Value as Json, json};

use super::NAME;
use super::store::Row;
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::port_types::Statement;
use crate::{Deployment, Error, Result};

/// Gate SQL appended to a change: runs only if the audit row exists.
pub(crate) const GATED: &str = "EXISTS (SELECT 1 FROM audit_log WHERE id = ?)";

pub(crate) fn summary(row: &Row) -> Json {
    json!({
        "status": row.status, "round": row.round, "version": row.version,
        "audience": row.audience, "require_ack": row.require_ack,
        "expires_at": row.expires_at, "send_at": row.send_at,
    })
}

/// One compare-and-set write: the audit row (only while `id` is at
/// `version` in one of `from`), then `changes(gate)`, then `ungated`
/// (Job inserts, which harmlessly do nothing when the race was lost:
/// their handlers check the version).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn cas(
    d: &Deployment,
    row: &Row,
    from: &[&str],
    action: &str,
    after: Json,
    actor: &Actor,
    ctx: &RequestCtx,
    changes: impl FnOnce(&str) -> Vec<Statement>,
    ungated: Vec<Statement>,
) -> Result<()> {
    if !from.contains(&row.status.as_str()) {
        return Err(Error::Conflict(format!("announcement is {}", row.status)));
    }
    let s = d.services();
    let marks = vec!["?"; from.len()].join(", ");
    let condition = from.iter().fold(
        Statement::new(format!(
            "EXISTS (SELECT 1 FROM announcement WHERE id = ? AND version = ? AND status IN ({marks}))"
        ))
        .bind(row.id.as_str())
        .bind(row.version),
        |st, f| st.bind(*f),
    );
    let target = Target::new(NAME, &row.id);
    let (entry, gate) = audit::entry_if(
        s,
        actor,
        action,
        Some(&target),
        Some(summary(row)),
        Some(after),
        ctx,
        condition,
    )?;
    let mut stmts = vec![entry];
    stmts.extend(changes(&gate));
    stmts.extend(ungated);
    let results = s.db.batch(stmts).await?;
    if results.first().map(|r| r.rows_affected) != Some(1) {
        return Err(Error::Conflict("announcement changed meanwhile".into()));
    }
    Ok(())
}

/// `UPDATE announcement SET <set>, version = version + 1, updated_at = ?`
/// gated; `set` binds come first.
pub(crate) fn bump(set: &str, row: &Row, now: i64, gate: &str) -> Statement {
    Statement::new(format!(
        "UPDATE announcement SET {set}, version = version + 1, updated_at = {now} \
         WHERE id = ? AND {GATED}"
    ))
    .bind(row.id.as_str())
    .bind(gate)
}

pub(crate) fn load_err(row: Option<Row>) -> Result<Row> {
    row.ok_or_else(|| Error::Invalid("no such announcement".into()))
}
