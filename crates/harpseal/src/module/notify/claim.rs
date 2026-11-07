//! The per-Student claim of a Notice (#73, migration 0014): one call at a
//! time sends to a (key, Student). A call claims the row with one
//! conditional upsert and goes on only when the row then carries its own
//! token; every later write is guarded by that token.

use crate::port_types::Statement;
use crate::{Deployment, Result};

/// How long a `sending` claim holds before another call may retake it
/// (the run that took it was cut off). Shorter than the Job lock (15
/// minutes), so the Job's own retry finds it free; longer than any one
/// notify call.
pub(super) const LEASE_MS: i64 = 10 * 60_000;
/// Rows untouched this long are pruned (a key reused after that sends
/// again).
pub(super) const KEEP_MS: i64 = 90 * 86_400_000;
/// Rows pruned per call.
const PRUNE: i64 = 100;

/// A claim this call holds.
pub(super) struct Claim {
    key: String,
    student: String,
    token: String,
}

/// What [`claim`] found.
pub(super) enum Claimed {
    /// This call holds it; `email` when an email may already be with the
    /// provider (a cut-off run had set `via = 'email'`).
    Mine(Claim, bool),
    /// Done by an earlier call, by `via`.
    Done(String),
    /// Another call holds it.
    Busy,
}

fn text(rows: &crate::port_types::Rows, col: &str) -> Option<String> {
    rows.get(0, col)
        .and_then(|v| v.as_text())
        .map(str::to_string)
}

/// Claims (scoped `key`, `student`): inserts the row, or retakes an
/// `open` one or a `sending` one whose lease ran out.
pub(super) async fn claim(
    d: &Deployment,
    key: &str,
    module: &str,
    student: &str,
) -> Result<Claimed> {
    let s = d.services();
    let now = s.now_ms();
    let token = s.new_id()?;
    let stmt = Statement::new(
        "INSERT INTO notify_delivery (notify_key, student_id, module, via, status, claim_token, \
         claimed_at, created_at, updated_at) VALUES (?, ?, ?, NULL, 'sending', ?, ?, ?, ?) \
         ON CONFLICT (notify_key, student_id) DO UPDATE SET status = 'sending', \
         claim_token = excluded.claim_token, claimed_at = excluded.claimed_at, \
         updated_at = excluded.updated_at WHERE notify_delivery.status = 'open' \
         OR (notify_delivery.status = 'sending' AND notify_delivery.claimed_at <= ?)",
    )
    .bind(key)
    .bind(student)
    .bind(module)
    .bind(token.as_str())
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(now - LEASE_MS);
    s.db.execute(stmt).await?;
    let rows =
        s.db.query(
            Statement::new(
                "SELECT via, status, claim_token FROM notify_delivery WHERE notify_key = ? \
                 AND student_id = ?",
            )
            .bind(key)
            .bind(student),
        )
        .await?;
    let via = text(&rows, "via");
    if text(&rows, "claim_token").as_deref() == Some(token.as_str())
        && text(&rows, "status").as_deref() == Some("sending")
    {
        let claim = Claim {
            key: key.into(),
            student: student.into(),
            token,
        };
        return Ok(Claimed::Mine(claim, via.as_deref() == Some("email")));
    }
    if text(&rows, "status").as_deref() == Some("done") {
        return Ok(Claimed::Done(via.unwrap_or_default()));
    }
    Ok(Claimed::Busy)
}

impl Claim {
    fn guarded(&self, set: &str) -> Statement {
        Statement::new(format!(
            "UPDATE notify_delivery SET {set} WHERE notify_key = ? AND student_id = ? \
             AND claim_token = ? AND status = 'sending'"
        ))
    }

    fn bind_key(&self, stmt: Statement) -> Statement {
        stmt.bind(self.key.as_str())
            .bind(self.student.as_str())
            .bind(self.token.as_str())
    }

    /// Records that `via` is about to go out; `false` when the claim was
    /// lost (its lease ran out and another call took it): send nothing.
    pub(super) async fn start(&self, d: &Deployment, via: &str) -> Result<bool> {
        let s = d.services();
        let stmt = self
            .guarded("via = ?, claimed_at = ?, updated_at = ?")
            .bind(via)
            .bind(s.now_ms())
            .bind(s.now_ms());
        let done = s.db.execute(self.bind_key(stmt)).await?;
        Ok(done.rows_affected == 1)
    }

    /// `done` by `via`.
    pub(super) async fn finish(&self, d: &Deployment, via: &str) -> Result<()> {
        let s = d.services();
        let stmt = self
            .guarded("via = ?, status = 'done', updated_at = ?")
            .bind(via)
            .bind(s.now_ms());
        s.db.execute(self.bind_key(stmt)).await.map(|_| ())
    }

    /// `open` again with nothing in flight: the next call retakes it at
    /// once.
    pub(super) async fn release(&self, d: &Deployment) -> Result<()> {
        let s = d.services();
        let stmt = self
            .guarded("via = NULL, status = 'open', updated_at = ?")
            .bind(s.now_ms());
        s.db.execute(self.bind_key(stmt)).await.map(|_| ())
    }
}

/// The `done` rows of these Students for `key`, by Student: via.
pub(super) async fn done(
    d: &Deployment,
    key: &str,
    ids: &[&str],
) -> Result<std::collections::HashMap<String, String>> {
    let mut out = std::collections::HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let mut stmt = Statement::new(format!(
        "SELECT student_id, via FROM notify_delivery WHERE notify_key = ? AND status = 'done' \
         AND student_id IN ({})",
        vec!["?"; ids.len()].join(", ")
    ))
    .bind(key);
    for id in ids {
        stmt = stmt.bind(*id);
    }
    let rows = d.services().db.query(stmt).await?;
    for i in 0..rows.len() {
        let col = |c: &str| {
            rows.get(i, c)
                .and_then(|v| v.as_text())
                .unwrap_or_default()
                .to_string()
        };
        out.insert(col("student_id"), col("via"));
    }
    Ok(out)
}

/// Deletes up to 100 rows untouched for [`KEEP_MS`].
pub(super) async fn prune(d: &Deployment) -> Result<()> {
    let s = d.services();
    let stmt = Statement::new(
        "DELETE FROM notify_delivery WHERE rowid IN (SELECT rowid FROM notify_delivery \
         WHERE updated_at < ? AND status <> 'sending' LIMIT ?)",
    )
    .bind(s.now_ms() - KEEP_MS)
    .bind(PRUNE);
    s.db.execute(stmt).await.map(|_| ())
}

/// Whether one account's `outbound_message` rows of `batch_key` record
/// all `n` Messages wholly sent (a chat a cut-off run finished, maybe on
/// a Link since revoked).
pub(super) async fn chat_on_record(d: &Deployment, batch_key: &str, n: usize) -> Result<bool> {
    let n = i64::try_from(n).unwrap_or(i64::MAX);
    let stmt = Statement::new(
        "SELECT 1 AS x FROM outbound_message WHERE batch_key = ? AND msg_index IS NOT NULL \
         AND msg_index < ? AND error IS NULL GROUP BY channel_account_id \
         HAVING count(DISTINCT msg_index) >= ? LIMIT 1",
    )
    .bind(batch_key)
    .bind(n)
    .bind(n);
    Ok(!d.services().db.query(stmt).await?.is_empty())
}
