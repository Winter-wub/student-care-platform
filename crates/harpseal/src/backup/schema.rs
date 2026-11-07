//! What the database holds, read through the [`Db`] port only, so the same
//! code works on rusqlite and D1: the tables, their columns, foreign keys
//! and triggers, from `sqlite_master` and `PRAGMA`s.

use std::collections::{BTreeMap, BTreeSet};

use crate::port_types::{Statement, Value};
use crate::{Db, Result};

/// Every user table: SQLite's own (`sqlite_%`), D1's (`_cf_%`, `d1_%`)
/// and the `migration` table (restored by re-running migrations) left out.
/// `substr`, not `LIKE`: `_` is a `LIKE` wildcard, so `'_cf_%'` would also
/// skip a table named `acf_x`.
const TABLES: &str = "SELECT name, sql FROM sqlite_master WHERE type = 'table' \
     AND substr(name, 1, 7) <> 'sqlite_' AND substr(name, 1, 4) <> '_cf_' \
     AND substr(name, 1, 3) <> 'd1_' AND name <> 'migration' ORDER BY name";

/// One table as the database describes it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Table {
    pub name: String,
    /// `sqlite_master.sql`.
    pub sql: String,
    pub columns: Vec<Column>,
    /// Tables this one references (self-references left out).
    pub parents: BTreeSet<String>,
}

/// One column (`PRAGMA table_info`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Column {
    pub name: String,
    pub not_null: bool,
    pub has_default: bool,
    /// Position in the primary key, 0 when not part of it.
    pub pk: i64,
}

impl Table {
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

/// Names of the triggers on `tables` that fire on `INSERT` (`BEFORE`,
/// `AFTER` or `INSTEAD OF INSERT ON ...`): a restore's inserts would set
/// them off.
pub(crate) async fn insert_triggers(db: &dyn Db, tables: &[String]) -> Result<Vec<String>> {
    let rows = db
        .query(Statement::new(
            "SELECT name, tbl_name, sql FROM sqlite_master WHERE type = 'trigger' ORDER BY name",
        ))
        .await?;
    Ok((0..rows.len())
        .filter(|&i| tables.contains(&text(rows.get(i, "tbl_name"))))
        .filter(|&i| fires_on_insert(&text(rows.get(i, "sql"))))
        .map(|i| text(rows.get(i, "name")))
        .collect())
}

/// Whether a `CREATE TRIGGER` statement's event is `INSERT`: the words
/// `INSERT ON` appear only in the event clause (`INSERT INTO` in a body
/// does not match).
fn fires_on_insert(sql: &str) -> bool {
    let sql = sql.to_ascii_uppercase();
    let words: Vec<&str> = sql.split_whitespace().collect();
    words.windows(2).any(|w| w == ["INSERT", "ON"])
}

/// Rows in `t` and the bytes of its values (text and blobs by length,
/// numbers by their text), in one scan: what a dump of it will hold.
pub(crate) async fn size(db: &dyn Db, t: &Table) -> Result<(u64, u64)> {
    let bytes: Vec<String> = t
        .columns
        .iter()
        .map(|c| format!("total(length(CAST({} AS BLOB)))", quote(&c.name)))
        .collect();
    let sum = if bytes.is_empty() {
        "0".to_string()
    } else {
        bytes.join(" + ")
    };
    let rows = db
        .query(Statement::new(format!(
            "SELECT count(*) AS n, {sum} AS b FROM {}",
            quote(&t.name)
        )))
        .await?;
    let n = rows.get(0, "n").and_then(Value::as_i64).unwrap_or(0).max(0) as u64;
    let b = match rows.get(0, "b") {
        Some(Value::Real(x)) => *x as u64,
        Some(Value::Integer(i)) => (*i).max(0) as u64,
        _ => 0,
    };
    Ok((n, b))
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
    fn insert_triggers_are_told_apart_from_bodies_that_insert() {
        assert!(fires_on_insert(
            "CREATE TRIGGER t AFTER INSERT ON tag BEGIN SELECT 1; END"
        ));
        assert!(fires_on_insert(
            "create trigger t before insert\non tag begin select 1; end"
        ));
        assert!(!fires_on_insert(
            "CREATE TRIGGER t AFTER UPDATE ON tag BEGIN INSERT INTO log VALUES (1); END"
        ));
        assert!(!fires_on_insert(
            "CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log BEGIN \
             SELECT RAISE(ABORT, 'x'); END"
        ));
    }

    #[test]
    fn identifiers_are_quoted() {
        assert_eq!(quote("student"), "\"student\"");
        assert_eq!(quote("we\"ird"), "\"we\"\"ird\"");
    }
}
