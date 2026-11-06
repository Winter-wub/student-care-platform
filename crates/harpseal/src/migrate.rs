//! Applies `.sql` migrations through the [`Db`] port and records each one,
//! with a checksum of its text, in the `migration` table: a second run is a
//! no-op, and a file edited after it was applied is refused. Works the same
//! on rusqlite and D1: each file is split into single statements and
//! applied as one all-or-nothing [`Db::batch`] together with its row.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::id::hex;
use crate::manifest::Migration;
use crate::port_types::{Statement, Value};
use crate::sql_split::split_sql;
use crate::{Db, Error, Result};

const CREATE_MIGRATION_TABLE: &str = "CREATE TABLE IF NOT EXISTS migration (\
     module TEXT NOT NULL, \
     name TEXT NOT NULL, \
     checksum TEXT NOT NULL, \
     applied_at INTEGER NOT NULL, \
     PRIMARY KEY (module, name))";

/// What [`migrate`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MigrationReport {
    /// `(module, name)` of each migration applied by this run, in order.
    pub applied: Vec<(String, String)>,
    /// How many were already recorded (with an unchanged checksum) and skipped.
    pub skipped: usize,
}

/// SHA-256 hex of the migration text, line endings normalised to `\n` so a
/// CRLF checkout does not count as an edit.
pub(crate) fn checksum(sql: &str) -> String {
    hex(&Sha256::digest(sql.replace("\r\n", "\n").as_bytes()))
}

/// Applies every migration not yet recorded in `migration`, in the order
/// given; `now_ms` is stored as `applied_at`. Fails, before applying
/// anything, when an already-applied file's text has changed.
pub async fn migrate(
    db: &dyn Db,
    migrations: &[Migration],
    now_ms: i64,
) -> Result<MigrationReport> {
    db.execute(Statement::new(CREATE_MIGRATION_TABLE)).await?;
    let rows = db
        .query(Statement::new(
            "SELECT module, name, checksum FROM migration",
        ))
        .await?;
    let done: HashMap<(String, String), String> = rows
        .rows
        .iter()
        .filter_map(|r| match r.as_slice() {
            [Value::Text(m), Value::Text(n), Value::Text(c)] => {
                Some(((m.clone(), n.clone()), c.clone()))
            }
            _ => None,
        })
        .collect();
    for m in migrations {
        let key = (m.module.to_string(), m.name.to_string());
        if let Some(applied) = done.get(&key)
            && *applied != checksum(m.sql)
        {
            return Err(Error::Db(format!(
                "migration {}/{} changed after it was applied (checksum {applied}, now {}); \
                 add a new migration instead of editing an applied one",
                m.module,
                m.name,
                checksum(m.sql)
            )));
        }
    }
    let mut report = MigrationReport::default();
    for m in migrations {
        let key = (m.module.to_string(), m.name.to_string());
        if done.contains_key(&key) {
            report.skipped += 1;
            continue;
        }
        let mut batch: Vec<Statement> = split_sql(m.sql).into_iter().map(Statement::new).collect();
        batch.push(
            Statement::new(
                "INSERT INTO migration (module, name, checksum, applied_at) VALUES (?, ?, ?, ?)",
            )
            .bind(m.module)
            .bind(m.name)
            .bind(checksum(m.sql))
            .bind(now_ms),
        );
        db.batch(batch)
            .await
            .map_err(|e| Error::Db(format!("migration {}/{} failed: {e}", m.module, m.name)))?;
        report.applied.push(key);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::checksum;

    #[test]
    fn checksum_ignores_line_endings_only() {
        assert_eq!(checksum("A;\r\nB;"), checksum("A;\nB;"));
        assert_ne!(checksum("A;"), checksum("A; "));
        assert_eq!(checksum("").len(), 64);
    }
}
