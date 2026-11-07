//! [`dump`]: the database and the FileStore objects it references, to a
//! [`BackupSink`].

use serde_json::{Map, Value as Json};

use super::progress::{Budget, DumpProgress, Step};
use super::schema::{Table, quote};
use super::value::encode;
use super::{BackupSink, DumpReport, ManifestMigration, PAGE};
use crate::port_types::{Rows, Statement, Value};
use crate::{Db, Deployment, Error, Result};

/// Writes a backup of `d` to `sink`: one `tables/<t>.ndjson` per table
/// (children before parents, see the module docs), one
/// `files/<hex>` per referenced FileStore object, then `manifest.json`
/// last (a sink without a manifest holds no finished backup). Reads in
/// pages of [`PAGE`] rows; each table's entry is built in memory before it
/// is written. Not one transaction: see the module docs. The nightly
/// backup Job runs the same dump in budgeted steps.
pub async fn dump(d: &Deployment, sink: &mut dyn BackupSink) -> Result<DumpReport> {
    let mut progress = DumpProgress::start(d).await?;
    match progress.step(d, sink, Budget::UNLIMITED).await? {
        Step::Done(report) => Ok(report),
        Step::More => Err(Error::Invalid("an unlimited dump stopped early".into())),
    }
}

/// The FileStore keys `t`'s `file_key` / `*_file_key` columns name.
pub(super) async fn file_keys(db: &dyn Db, t: &Table) -> Result<Vec<String>> {
    let mut keys = Vec::new();
    for col in t.file_key_columns() {
        let found = db
            .query(Statement::new(format!(
                "SELECT DISTINCT {c} AS k FROM {t} WHERE {c} IS NOT NULL",
                c = quote(col),
                t = quote(&t.name)
            )))
            .await?;
        keys.extend(found.rows.iter().filter_map(|r| match r.first() {
            Some(Value::Text(k)) if !k.is_empty() => Some(k.clone()),
            _ => None,
        }));
    }
    Ok(keys)
}

/// The `migration` table in applied (insertion) order.
pub(super) async fn applied_migrations(db: &dyn Db) -> Result<Vec<ManifestMigration>> {
    let rows = db
        .query(Statement::new(
            "SELECT module, name, checksum FROM migration ORDER BY rowid",
        ))
        .await
        .map_err(|e| Error::Db(format!("no migration table (run `harpseal migrate`?): {e}")))?;
    let text = |i: usize, c: &str| {
        rows.get(i, c)
            .and_then(Value::as_text)
            .unwrap_or_default()
            .to_string()
    };
    Ok((0..rows.len())
        .map(|i| ManifestMigration {
            module: text(i, "module"),
            name: text(i, "name"),
            checksum: text(i, "checksum"),
        })
        .collect())
}

/// Column the rowid is read into, ahead of `*`.
const ROWID: &str = "__harpseal_rowid";

/// Every row of `t` as NDJSON, and how many rows there were.
pub(super) async fn dump_table(db: &dyn Db, t: &Table) -> Result<(Vec<u8>, u64)> {
    let mut out = Vec::new();
    let mut count = 0u64;
    read_pages(db, t, |page, skip| {
        count += write_rows(&mut out, page, skip)?;
        Ok(true)
    })
    .await?;
    Ok((out, count))
}

/// Reads every row of `t` in pages of [`PAGE`], in `rowid` order (primary
/// key order for `WITHOUT ROWID` tables), handing each page to `f` with
/// the number of leading columns that are not the table's (the rowid).
/// `f` returns `false` to stop early.
pub(super) async fn read_pages(
    db: &dyn Db,
    t: &Table,
    mut f: impl FnMut(&Rows, usize) -> Result<bool>,
) -> Result<()> {
    if t.without_rowid() {
        let order: Vec<String> = t.pk_columns().iter().map(|c| quote(c)).collect();
        let mut offset = 0i64;
        loop {
            let page = db
                .query(
                    Statement::new(format!(
                        "SELECT * FROM {} ORDER BY {} LIMIT {PAGE} OFFSET ?",
                        quote(&t.name),
                        order.join(", ")
                    ))
                    .bind(offset),
                )
                .await?;
            if !f(&page, 0)? || page.len() < PAGE {
                return Ok(());
            }
            offset += PAGE as i64;
        }
    }
    // The first page has no lower bound (a rowid can be i64::MIN).
    let mut after: Option<i64> = None;
    loop {
        let head = format!("SELECT rowid AS {ROWID}, * FROM {}", quote(&t.name));
        let stmt = match after {
            None => Statement::new(format!("{head} ORDER BY rowid LIMIT {PAGE}")),
            Some(a) => Statement::new(format!(
                "{head} WHERE rowid > ? ORDER BY rowid LIMIT {PAGE}"
            ))
            .bind(a),
        };
        let page = db.query(stmt).await?;
        if !f(&page, 1)? {
            return Ok(());
        }
        match page
            .rows
            .last()
            .and_then(|r| r.first())
            .and_then(Value::as_i64)
        {
            Some(last) if page.len() == PAGE => after = Some(last),
            _ => return Ok(()),
        }
    }
}

/// Appends one JSON line per row, skipping the first `skip` columns.
fn write_rows(out: &mut Vec<u8>, page: &Rows, skip: usize) -> Result<u64> {
    for row in &page.rows {
        let obj: Map<String, Json> = page
            .columns
            .iter()
            .zip(row)
            .skip(skip)
            .map(|(c, v)| (c.clone(), encode(v)))
            .collect();
        serde_json::to_writer(&mut *out, &Json::Object(obj))
            .map_err(|e| Error::Invalid(format!("backup row: {e}")))?;
        out.push(b'\n');
    }
    Ok(page.rows.len() as u64)
}
