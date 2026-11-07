//! [`verify`]: the restore's last check before it writes anything. The
//! decoded rows must insert cleanly: no two rows share a primary key or a
//! (non-partial) `UNIQUE` index, and every foreign key resolves among the
//! rows of the backup.
//!
//! A backup taken while the Deployment writes (the nightly one) may hold a
//! child row whose parent was deleted after the child's table was read,
//! when the delete cascaded (a Role and its grants). Such a row would have
//! been deleted with its parent, so it is dropped (and counted), repeatedly
//! until nothing else hangs off a dropped row. A dangling reference
//! through a key without `ON DELETE CASCADE`, or a duplicate key, refuses
//! the restore with a report and nothing written.

use std::collections::{BTreeMap, HashSet};

use super::schema::{Table, quote};
use crate::port_types::{Statement, Value};
use crate::{Db, Error, Result};

/// Problems listed in a refusal.
const SHOWN: usize = 10;

/// One foreign key of a table.
struct Fk {
    parent: String,
    from: Vec<String>,
    to: Vec<String>,
    cascade: bool,
}

/// Rows of one table, by column name.
pub(super) struct Decoded<'a> {
    pub(super) columns: &'a [String],
    pub(super) rows: Vec<Vec<Value>>,
}

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_text).unwrap_or_default().to_string()
}

/// A value as a comparable string, `None` for `NULL` (never matches).
fn key_part(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::Integer(i) => Some(format!("i{i}")),
        Value::Real(x) if x.fract() == 0.0 && x.abs() < 9e15 => Some(format!("i{}", *x as i64)),
        Value::Real(x) => Some(format!("r{x}")),
        Value::Text(s) => Some(format!("t{s}")),
        Value::Blob(b) => Some(format!("b{}", crate::id::hex(b))),
    }
}

/// The tuple of `cols` in `row`, `None` if any is `NULL` or missing.
fn tuple(d: &Decoded<'_>, row: &[Value], cols: &[String]) -> Option<String> {
    let mut out = String::new();
    for c in cols {
        let i = d.columns.iter().position(|n| n == c)?;
        out.push_str(&key_part(row.get(i)?)?);
        out.push('\u{1f}');
    }
    Some(out)
}

async fn foreign_keys(db: &dyn Db, t: &Table, tables: &[Table]) -> Result<Vec<Fk>> {
    let rows = db
        .query(Statement::new(format!(
            "PRAGMA foreign_key_list({})",
            quote(&t.name)
        )))
        .await?;
    let mut by_id: BTreeMap<i64, Fk> = BTreeMap::new();
    for i in 0..rows.len() {
        let id = rows.get(i, "id").and_then(Value::as_i64).unwrap_or(0);
        let parent = text(rows.get(i, "table"));
        let fk = by_id.entry(id).or_insert_with(|| Fk {
            parent: parent.clone(),
            from: Vec::new(),
            to: Vec::new(),
            cascade: text(rows.get(i, "on_delete")).eq_ignore_ascii_case("CASCADE"),
        });
        fk.from.push(text(rows.get(i, "from")));
        fk.to.push(text(rows.get(i, "to")));
    }
    let mut out: Vec<Fk> = by_id.into_values().collect();
    // `REFERENCES parent` without columns names the parent's primary key.
    for fk in &mut out {
        if fk.to.iter().all(String::is_empty)
            && let Some(p) = tables.iter().find(|p| p.name == fk.parent)
        {
            fk.to = p.pk_columns().iter().map(|c| c.to_string()).collect();
        }
    }
    Ok(out)
}

/// The primary key and every non-partial `UNIQUE` index of `t`, as columns.
async fn unique_keys(db: &dyn Db, t: &Table) -> Result<Vec<Vec<String>>> {
    let mut keys = Vec::new();
    let pk: Vec<String> = t.pk_columns().iter().map(|c| c.to_string()).collect();
    if !pk.is_empty() {
        keys.push(pk);
    }
    let list = db
        .query(Statement::new(format!(
            "PRAGMA index_list({})",
            quote(&t.name)
        )))
        .await?;
    for i in 0..list.len() {
        let unique = list.get(i, "unique").and_then(Value::as_i64) == Some(1);
        let partial = list.get(i, "partial").and_then(Value::as_i64) == Some(1);
        let origin = text(list.get(i, "origin"));
        if !unique || partial || origin == "pk" {
            continue;
        }
        let name = text(list.get(i, "name"));
        let info = db
            .query(Statement::new(format!(
                "PRAGMA index_info({})",
                quote(&name)
            )))
            .await?;
        let cols: Vec<String> = (0..info.len()).map(|r| text(info.get(r, "name"))).collect();
        // An index on an expression names no column: not checked here.
        if !cols.is_empty() && cols.iter().all(|c| !c.is_empty()) {
            keys.push(cols);
        }
    }
    Ok(keys)
}

/// Checks `decoded` (table name to rows) against the schema of `tables`;
/// drops rows orphaned through a cascading key and returns how many per
/// table, or refuses.
pub(super) async fn verify(
    db: &dyn Db,
    tables: &[Table],
    decoded: &mut BTreeMap<String, Decoded<'_>>,
) -> Result<BTreeMap<String, u64>> {
    let mut fks = BTreeMap::new();
    let mut uniques = BTreeMap::new();
    for t in tables.iter().filter(|t| decoded.contains_key(&t.name)) {
        fks.insert(t.name.clone(), foreign_keys(db, t, tables).await?);
        uniques.insert(t.name.clone(), unique_keys(db, t).await?);
    }
    let mut problems = Vec::new();
    let mut dropped: BTreeMap<String, u64> = BTreeMap::new();
    loop {
        let mut changed = false;
        for (name, keys) in &fks {
            for fk in keys {
                let Some(parent) = decoded.get(&fk.parent) else {
                    continue;
                };
                let present: HashSet<String> = parent
                    .rows
                    .iter()
                    .filter_map(|r| tuple(parent, r, &fk.to))
                    .collect();
                let child = &decoded[name];
                let orphan: Vec<bool> = child
                    .rows
                    .iter()
                    .map(|r| tuple(child, r, &fk.from).is_some_and(|k| !present.contains(&k)))
                    .collect();
                let n = orphan.iter().filter(|o| **o).count();
                if n == 0 {
                    continue;
                }
                if !fk.cascade {
                    problems.push(format!(
                        "{name}: {n} rows reference {} ({}) rows that are not in the backup",
                        fk.parent,
                        fk.from.join(", ")
                    ));
                    continue;
                }
                let child = decoded.get_mut(name).expect("checked above");
                let mut it = orphan.into_iter();
                child.rows.retain(|_| !it.next().unwrap_or(false));
                *dropped.entry(name.clone()).or_default() += n as u64;
                changed = true;
            }
        }
        if !changed || !problems.is_empty() {
            break;
        }
    }
    for (name, keys) in &uniques {
        let d = &decoded[name];
        for cols in keys {
            let mut seen = HashSet::new();
            let dup = d
                .rows
                .iter()
                .filter_map(|r| tuple(d, r, cols))
                .filter(|k| !seen.insert(k.clone()))
                .count();
            if dup > 0 {
                problems.push(format!("{name}: {dup} rows repeat ({})", cols.join(", ")));
            }
        }
    }
    if problems.is_empty() {
        return Ok(dropped);
    }
    let more = problems.len().saturating_sub(SHOWN);
    problems.truncate(SHOWN);
    let tail = if more > 0 {
        format!("; and {more} more")
    } else {
        String::new()
    };
    Err(Error::Invalid(format!(
        "the backup would not insert cleanly, so nothing was restored: {}{tail}",
        problems.join("; ")
    )))
}
