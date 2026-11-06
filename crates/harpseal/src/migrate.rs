//! Applies `.sql` migrations through the [`Db`] port and records each one in
//! the `migration` table, so a second run is a no-op. Works the same on
//! rusqlite and D1: each file is split into single statements and applied
//! as one all-or-nothing [`Db::batch`] together with its `migration` row.

use std::collections::HashSet;

use crate::manifest::Migration;
use crate::port_types::{Statement, Value};
use crate::{Db, Error, Result};

const CREATE_MIGRATION_TABLE: &str = "CREATE TABLE IF NOT EXISTS migration (\
     module TEXT NOT NULL, \
     name TEXT NOT NULL, \
     applied_at INTEGER NOT NULL, \
     PRIMARY KEY (module, name))";

/// What [`migrate`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MigrationReport {
    /// `(module, name)` of each migration applied by this run, in order.
    pub applied: Vec<(String, String)>,
    /// How many were already recorded and skipped.
    pub skipped: usize,
}

/// Applies every migration not yet recorded in `migration`, in the order
/// given. `now_ms` is stored as `applied_at`.
pub async fn migrate(
    db: &dyn Db,
    migrations: &[Migration],
    now_ms: i64,
) -> Result<MigrationReport> {
    db.execute(Statement::new(CREATE_MIGRATION_TABLE)).await?;
    let rows = db
        .query(Statement::new("SELECT module, name FROM migration"))
        .await?;
    let done: HashSet<(String, String)> = rows
        .rows
        .iter()
        .filter_map(|r| match r.as_slice() {
            [Value::Text(m), Value::Text(n)] => Some((m.clone(), n.clone())),
            _ => None,
        })
        .collect();
    let mut report = MigrationReport::default();
    for m in migrations {
        let key = (m.module.to_string(), m.name.to_string());
        if done.contains(&key) {
            report.skipped += 1;
            continue;
        }
        let mut batch: Vec<Statement> = split_sql(m.sql).into_iter().map(Statement::new).collect();
        batch.push(
            Statement::new("INSERT INTO migration (module, name, applied_at) VALUES (?, ?, ?)")
                .bind(m.module)
                .bind(m.name)
                .bind(now_ms),
        );
        db.batch(batch)
            .await
            .map_err(|e| Error::Db(format!("migration {}/{} failed: {e}", m.module, m.name)))?;
        report.applied.push(key);
    }
    Ok(report)
}

/// Splits a SQL file into statements at top-level `;`. Understands `--` and
/// `/* */` comments, quoted strings and identifiers, and `CREATE TRIGGER ...
/// BEGIN ... END;` bodies. Comment-only fragments are dropped.
pub(crate) fn split_sql(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '-' if chars.peek() == Some(&'-') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        cur.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
                cur.push(' ');
            }
            '\'' | '"' | '`' => {
                cur.push(c);
                for q in chars.by_ref() {
                    cur.push(q);
                    if q == c {
                        break;
                    }
                }
            }
            ';' if !inside_trigger_body(&cur) => push_statement(&mut out, &mut cur),
            _ => cur.push(c),
        }
    }
    push_statement(&mut out, &mut cur);
    out
}

/// A `;` inside `CREATE TRIGGER ... BEGIN` ends a body statement, not the
/// trigger; the trigger ends at the `;` after `END`.
fn inside_trigger_body(cur: &str) -> bool {
    let upper = cur.trim_start().to_ascii_uppercase();
    upper.starts_with("CREATE TRIGGER") && !upper.trim_end().ends_with("END")
}

fn push_statement(out: &mut Vec<String>, cur: &mut String) {
    let s = cur.trim();
    if !s.is_empty() {
        out.push(s.to_string());
    }
    cur.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_statements_and_skips_comments() {
        let sql = "-- header; not a statement\nCREATE TABLE a (x TEXT DEFAULT ';');\n\
                   /* block; */ CREATE INDEX i ON a (x);\n-- trailing\n";
        assert_eq!(
            split_sql(sql),
            vec![
                "CREATE TABLE a (x TEXT DEFAULT ';')".to_string(),
                "CREATE INDEX i ON a (x)".to_string(),
            ]
        );
    }

    #[test]
    fn keeps_trigger_bodies_whole() {
        let sql =
            "CREATE TRIGGER t BEFORE DELETE ON a BEGIN SELECT RAISE(ABORT, 'no'); END;\nSELECT 1;";
        let parts = split_sql(sql);
        assert_eq!(parts.len(), 2);
        assert!(parts[0].ends_with("END"), "{parts:?}");
    }
}
