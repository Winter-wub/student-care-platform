//! What the database holds, read through the [`Db`] port only, so the same
//! code works on rusqlite and D1: the tables, their columns, foreign keys
//! and triggers, from `sqlite_master` and `PRAGMA`s.

use std::collections::{BTreeMap, BTreeSet};

use crate::port_types::{Statement, Value};
use crate::{Db, Result};

/// Every user table: SQLite's own (`sqlite_%`), D1's (`_cf_%`, `d1_%`)
/// and the `migration` table (restored by re-running migrations) left out.
const TABLES: &str = "SELECT name, sql FROM sqlite_master WHERE type = 'table' \
     AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_cf_%' AND name NOT LIKE 'd1_%' \
     AND name <> 'migration' ORDER BY name";

/// One table as the database describes it.
#[derive(Debug, Clone)]
pub(crate) struct Table {
    pub name: String,
    /// `sqlite_master.sql`.
    pub sql: String,
    pub columns: Vec<Column>,
    /// Tables this one references (self-references left out).
    pub parents: BTreeSet<String>,
}

/// One column (`PRAGMA table_info`).
#[derive(Debug, Clone)]
pub(crate) struct Column {
    pub name: String,
    pub not_null: bool,
    pub has_default: bool,
    /// Position in the primary key, 0 when not part of it.
    pub pk: i64,
}

impl Table {
    /// `WITHOUT ROWID` tables have no `rowid` to page by.
    pub fn without_rowid(&self) -> bool {
        // Table options follow the column list's closing parenthesis.
        let sql = self.sql.to_ascii_uppercase();
        let tail = sql.rsplit_once(')').map(|(_, t)| t).unwrap_or("");
        tail.replace(',', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|w| w == ["WITHOUT", "ROWID"])
    }

    /// Primary-key columns in key order.
    pub fn pk_columns(&self) -> Vec<&str> {
        let mut pk: Vec<&Column> = self.columns.iter().filter(|c| c.pk > 0).collect();
        pk.sort_by_key(|c| c.pk);
        pk.into_iter().map(|c| c.name.as_str()).collect()
    }

    /// Columns holding FileStore keys: `file_key` or `*_file_key`.
    pub fn file_key_columns(&self) -> Vec<&str> {
        self.columns
            .iter()
            .map(|c| c.name.as_str())
            .filter(|n| *n == "file_key" || n.ends_with("_file_key"))
            .collect()
    }
}

/// `"name"`, with embedded quotes doubled.
pub(crate) fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_text).unwrap_or_default().to_string()
}

/// Every user table with its columns and foreign keys, by name.
pub(crate) async fn tables(db: &dyn Db) -> Result<Vec<Table>> {
    let rows = db.query(Statement::new(TABLES)).await?;
    let mut out = Vec::with_capacity(rows.len());
    for i in 0..rows.len() {
        let name = text(rows.get(i, "name"));
        let sql = text(rows.get(i, "sql"));
        let info = db
            .query(Statement::new(format!(
                "PRAGMA table_info({})",
                quote(&name)
            )))
            .await?;
        let columns = (0..info.len())
            .map(|r| Column {
                name: text(info.get(r, "name")),
                not_null: info.get(r, "notnull").and_then(Value::as_i64) == Some(1),
                has_default: !matches!(info.get(r, "dflt_value"), None | Some(Value::Null)),
                pk: info.get(r, "pk").and_then(Value::as_i64).unwrap_or(0),
            })
            .collect();
        let fks = db
            .query(Statement::new(format!(
                "PRAGMA foreign_key_list({})",
                quote(&name)
            )))
            .await?;
        let parents = (0..fks.len())
            .map(|r| text(fks.get(r, "table")))
            .filter(|p| *p != name)
            .collect();
        out.push(Table {
            name,
            sql,
            columns,
            parents,
        });
    }
    Ok(out)
}

/// Table names parents first, so inserts in this order satisfy foreign
/// keys (deletes go in reverse). Ties and cycles fall back to name order.
pub(crate) fn dependency_order(tables: &[Table]) -> Vec<String> {
    let known: BTreeSet<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    let mut waiting: BTreeMap<&str, BTreeSet<&str>> = tables
        .iter()
        .map(|t| {
            let parents = t
                .parents
                .iter()
                .map(String::as_str)
                .filter(|p| known.contains(p))
                .collect();
            (t.name.as_str(), parents)
        })
        .collect();
    let mut order = Vec::with_capacity(tables.len());
    while !waiting.is_empty() {
        let ready: Vec<&str> = waiting
            .iter()
            .filter(|(_, ps)| ps.is_empty())
            .map(|(n, _)| *n)
            .collect();
        // A cycle: take the first remaining table by name.
        let ready = if ready.is_empty() {
            vec![*waiting.keys().next().expect("not empty")]
        } else {
            ready
        };
        for n in ready {
            waiting.remove(n);
            for ps in waiting.values_mut() {
                ps.remove(n);
            }
            order.push(n.to_string());
        }
    }
    order
}

/// `(name, sql)` of each trigger on `tables` whose SQL mentions `DELETE`
/// (the append-only guards a forced restore must lift for its wipe).
pub(crate) async fn delete_triggers(
    db: &dyn Db,
    tables: &[String],
) -> Result<Vec<(String, String)>> {
    let rows = db
        .query(Statement::new(
            "SELECT name, tbl_name, sql FROM sqlite_master WHERE type = 'trigger' ORDER BY name",
        ))
        .await?;
    Ok((0..rows.len())
        .filter(|&i| tables.contains(&text(rows.get(i, "tbl_name"))))
        .map(|i| (text(rows.get(i, "name")), text(rows.get(i, "sql"))))
        .filter(|(_, sql)| sql.to_ascii_uppercase().contains("DELETE"))
        .collect())
}

/// Rows in `table`.
pub(crate) async fn count(db: &dyn Db, table: &str) -> Result<u64> {
    let rows = db
        .query(Statement::new(format!(
            "SELECT count(*) AS n FROM {}",
            quote(table)
        )))
        .await?;
    Ok(rows.get(0, "n").and_then(Value::as_i64).unwrap_or(0).max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(name: &str, parents: &[&str], sql: &str) -> Table {
        Table {
            name: name.into(),
            sql: sql.into(),
            columns: Vec::new(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
        }
    }

    #[test]
    fn parents_come_first() {
        let t = [
            table("link", &["student", "channel_account"], ""),
            table("student", &[], ""),
            table("channel_account", &[], ""),
            table("delivery", &["link", "missing"], ""),
        ];
        assert_eq!(
            dependency_order(&t),
            ["channel_account", "student", "link", "delivery"]
        );
        let cycle = [table("a", &["b"], ""), table("b", &["a"], "")];
        assert_eq!(dependency_order(&cycle), ["a", "b"]);
    }

    #[test]
    fn without_rowid_is_read_from_the_sql() {
        assert!(
            table(
                "t",
                &[],
                "CREATE TABLE t (k TEXT PRIMARY KEY) WITHOUT ROWID"
            )
            .without_rowid()
        );
        assert!(
            table("t", &[], "CREATE TABLE t (k TEXT PRIMARY KEY)without rowid").without_rowid()
        );
        assert!(!table("t", &[], "CREATE TABLE t (k TEXT PRIMARY KEY)").without_rowid());
        assert!(!table("t", &[], "CREATE TABLE t (without_rowid TEXT)").without_rowid());
    }

    #[test]
    fn identifiers_are_quoted() {
        assert_eq!(quote("student"), "\"student\"");
        assert_eq!(quote("we\"ird"), "\"we\"\"ird\"");
    }
}
