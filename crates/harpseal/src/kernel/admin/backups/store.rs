//! The `backup_run` table (migration 0013): one row per backup.

use crate::port_types::{Rows, Statement, Value};
use crate::{Result, Services};

/// Columns every read selects.
const COLUMNS: &str = "id, prefix, state, origin, requested_by, started_at, updated_at, \
     finished_at, progress, bytes, table_count, row_count, file_count, manifest_sha256, error";

/// One `backup_run` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Run {
    pub(super) id: String,
    pub(super) prefix: String,
    /// `running`, `done` or `failed`.
    pub(super) state: String,
    /// `schedule` or `manual`.
    pub(super) origin: String,
    pub(super) started_at: i64,
    pub(super) updated_at: i64,
    pub(super) finished_at: Option<i64>,
    /// Job runs so far.
    pub(super) progress: i64,
    pub(super) bytes: i64,
    pub(super) tables: Option<i64>,
    pub(super) rows: Option<i64>,
    pub(super) files: Option<i64>,
    pub(super) error: Option<String>,
}

fn runs(rows: &Rows) -> Vec<Run> {
    (0..rows.len())
        .map(|i| {
            let text = |c: &str| rows.get(i, c).and_then(Value::as_text).map(String::from);
            let int = |c: &str| rows.get(i, c).and_then(Value::as_i64);
            Run {
                id: text("id").unwrap_or_default(),
                prefix: text("prefix").unwrap_or_default(),
                state: text("state").unwrap_or_default(),
                origin: text("origin").unwrap_or_default(),
                started_at: int("started_at").unwrap_or(0),
                updated_at: int("updated_at").unwrap_or(0),
                finished_at: int("finished_at"),
                progress: int("progress").unwrap_or(0),
                bytes: int("bytes").unwrap_or(0),
                tables: int("table_count"),
                rows: int("row_count"),
                files: int("file_count"),
                error: text("error"),
            }
        })
        .collect()
}

/// The run `id`.
pub(super) async fn get(s: &Services, id: &str) -> Result<Option<Run>> {
    let sql = format!("SELECT {COLUMNS} FROM backup_run WHERE id = ?");
    let rows = s.db.query(Statement::new(sql).bind(id)).await?;
    Ok(runs(&rows).into_iter().next())
}

/// The newest `limit` runs, newest first.
pub(super) async fn list(s: &Services, limit: i64) -> Result<Vec<Run>> {
    let sql = format!("SELECT {COLUMNS} FROM backup_run ORDER BY started_at DESC, id DESC LIMIT ?");
    Ok(runs(&s.db.query(Statement::new(sql).bind(limit)).await?))
}

/// The run in progress, if any.
pub(super) async fn running(s: &Services) -> Result<Option<Run>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM backup_run WHERE state = 'running' \
         ORDER BY started_at LIMIT 1"
    );
    Ok(runs(&s.db.query(Statement::new(sql)).await?)
        .into_iter()
        .next())
}

/// Runs retention may delete: started before `before`, not running, and
/// not the newest `done` one. At most `limit`, oldest first.
pub(super) async fn expired(s: &Services, before: i64, limit: i64) -> Result<Vec<Run>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM backup_run WHERE started_at < ? AND state <> 'running' \
         AND id IS NOT (SELECT id FROM backup_run WHERE state = 'done' \
         ORDER BY started_at DESC, id DESC LIMIT 1) \
         ORDER BY started_at, id LIMIT ?"
    );
    Ok(runs(
        &s.db
            .query(Statement::new(sql).bind(before).bind(limit))
            .await?,
    ))
}

/// The `INSERT` of a new run, only while no other run is `running`.
pub(super) fn insert(
    s: &Services,
    id: &str,
    prefix: &str,
    origin: &str,
    requested_by: Option<&str>,
) -> Statement {
    let now = s.now_ms();
    Statement::new(
        "INSERT INTO backup_run (id, prefix, state, origin, requested_by, started_at, \
         updated_at) SELECT ?, ?, 'running', ?, ?, ?, ? \
         WHERE NOT EXISTS (SELECT 1 FROM backup_run WHERE state = 'running')",
    )
    .bind(id)
    .bind(prefix)
    .bind(origin)
    .bind(requested_by.map(String::from))
    .bind(now)
    .bind(now)
}

/// One more step saved: `progress + 1`, the bytes so far. `false` when the
/// run is no longer `running`.
pub(super) async fn advance(s: &Services, id: &str, bytes: u64) -> Result<bool> {
    let r =
        s.db.execute(
            Statement::new(
                "UPDATE backup_run SET progress = progress + 1, bytes = ?, updated_at = ? \
                 WHERE id = ? AND state = 'running'",
            )
            .bind(i64::try_from(bytes).unwrap_or(i64::MAX))
            .bind(s.now_ms())
            .bind(id),
        )
        .await?;
    Ok(r.rows_affected == 1)
}

/// Records `error` on a running run; with `fail`, also marks it `failed`.
/// `true` when the row changed.
pub(super) async fn note_error(s: &Services, id: &str, error: &str, fail: bool) -> Result<bool> {
    let now = s.now_ms();
    let r =
        s.db.execute(
            Statement::new(
                "UPDATE backup_run SET error = ?, updated_at = ?, \
                 state = CASE WHEN ? THEN 'failed' ELSE state END, \
                 finished_at = CASE WHEN ? THEN ? ELSE finished_at END \
                 WHERE id = ? AND state = 'running'",
            )
            .bind(crate::jobs::cut(error, 500))
            .bind(now)
            .bind(fail)
            .bind(fail)
            .bind(now)
            .bind(id),
        )
        .await?;
    Ok(r.rows_affected == 1)
}
