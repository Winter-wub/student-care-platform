//! [`read_pages`]: every row of a table, in pages of [`PAGE`], by keyset.
//!
//! **Keyset by primary key.** A table with a declared primary key is read
//! in primary-key order, each page starting after the last key read
//! (`WHERE (k1, k2) > (?, ?)`), so a row deleted and re-inserted with the
//! same key while the dump runs (roster Tag replacement, an edited
//! announcement text) is read at most once: paging by `rowid` would meet it
//! again under its new `rowid` and dump a duplicate key, which the restore
//! cannot insert. Only a table without a primary key is read by `rowid`.
//!
//! What keyset paging cannot see: a row whose primary key is **updated**
//! between two pages moves in the order, so it may be read twice (under
//! both keys) or not at all. No Kernel path updates a primary key except
//! the single-row `menu_salt`, which never spans pages. A `NULL` in a
//! primary key (SQLite allows it outside `INTEGER PRIMARY KEY` and `NOT
//! NULL`) would end the paging early, so it refuses the dump instead,
//! naming the table.
//!
//! **Transient rows.** Live Verification challenges (`pending` and not yet
//! expired) are never read: their salted hash of a short code is
//! crackable offline, and a challenge is worth nothing after a restore.
//! Kept Module uploads (`module_upload`, #74) are never read either: a
//! file waiting for its confirm is worth nothing after a restore, and
//! Staff did not mean it to be kept for the backups' 30 days.

use super::PAGE;
use super::schema::{Table, quote};
use crate::port_types::{Rows, Statement, Value};
use crate::{Db, Error, Result};

/// Column the rowid is read into, ahead of `*` (tables without a key).
const ROWID: &str = "__harpseal_rowid";

/// The rows of `table` a dump at `now` leaves out, as a `WHERE` condition
/// for the rows it keeps.
pub(crate) fn kept_rows(table: &str, now: i64) -> Option<Statement> {
    match table {
        "verification_challenge" => {
            Some(Statement::new("(state <> 'pending' OR expires_at <= ?)").bind(now))
        }
        "module_upload" => Some(Statement::new("0")),
        _ => None,
    }
}

/// Reads every row of `t` that a dump at `now` keeps, in pages of
/// [`PAGE`], handing each page to `f` with the number of leading columns
/// that are not the table's (1 for the rowid of a table without a key, else
/// 0). `f` returns `false` to stop early.
pub(crate) async fn read_pages(
    db: &dyn Db,
    t: &Table,
    now: i64,
    mut f: impl FnMut(&Rows, usize) -> Result<bool>,
) -> Result<()> {
    let pk = t.pk_columns();
    let (head, key, skip): (String, Vec<String>, usize) = if pk.is_empty() {
        let head = format!("SELECT rowid AS {ROWID}, * FROM {}", quote(&t.name));
        (head, vec![ROWID.to_string()], 1)
    } else {
        let head = format!("SELECT * FROM {}", quote(&t.name));
        (head, pk.iter().map(|c| c.to_string()).collect(), 0)
    };
    let order = if skip == 1 {
        "rowid".to_string()
    } else {
        key.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ")
    };
    let filter = kept_rows(&t.name, now);
    // The first page has no lower bound (a rowid can be i64::MIN).
    let mut after: Option<Vec<Value>> = None;
    loop {
        let mut conds = Vec::new();
        let mut params = Vec::new();
        if let Some(fl) = &filter {
            conds.push(fl.sql.to_string());
            params.extend(fl.params.iter().cloned());
        }
        if let Some(last) = &after {
            let marks = vec!["?"; last.len()].join(", ");
            conds.push(format!("({order}) > ({marks})"));
            params.extend(last.iter().cloned());
        }
        let cond = if conds.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conds.join(" AND "))
        };
        let sql = format!("{head}{cond} ORDER BY {order} LIMIT {PAGE}");
        let page = db
            .query(Statement {
                sql: sql.into(),
                params,
            })
            .await?;
        if skip == 0 {
            let idx: Vec<usize> = key
                .iter()
                .filter_map(|k| page.columns.iter().position(|c| c == k))
                .collect();
            let null = page.rows.iter().any(|r| {
                idx.iter()
                    .any(|&i| matches!(r.get(i), None | Some(Value::Null)))
            });
            if null {
                return Err(Error::Invalid(format!(
                    "table {} has a row with a NULL primary key ({}); it cannot be backed up \
                     page by page: give the row a key first",
                    t.name,
                    key.join(", ")
                )));
            }
        }
        if !f(&page, skip)? || page.len() < PAGE {
            return Ok(());
        }
        let Some(row) = page.rows.last() else {
            return Ok(());
        };
        let last = key
            .iter()
            .map(|k| {
                let i = page.columns.iter().position(|c| c == k);
                i.and_then(|i| row.get(i).cloned()).unwrap_or(Value::Null)
            })
            .collect();
        after = Some(last);
    }
}
