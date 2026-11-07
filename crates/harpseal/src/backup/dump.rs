//! [`dump`]: the database and the FileStore objects it references, to a
//! [`BackupSink`].

use serde_json::{Map, Value as Json};

use super::pages::read_pages;
use super::progress::{Budget, DumpProgress, Step};
use super::schema::{Table, quote};
use super::value::encode;
use super::{BackupSink, DumpReport, ManifestMigration};
use crate::port_types::{Rows, Statement, Value};
use crate::{Db, Deployment, Error, Result};

/// Writes a backup of `d` to `sink`: one `tables/<t>.ndjson` per table
/// (children before parents, see the module docs), one
/// `files/<hex>` per referenced FileStore object, then `manifest.json`
/// last (a sink without a manifest holds no finished backup). Reads in
/// pages of [`super::PAGE`] rows, by primary key; each table's entry is built in memory before it
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

/// Every row of `t` a dump at `now` keeps, as NDJSON, and how many rows
/// there were (see [`read_pages`] for the order).
pub(super) async fn dump_table(db: &dyn Db, t: &Table, now: i64) -> Result<(Vec<u8>, u64)> {
    let mut out = Vec::new();
    let mut count = 0u64;
    read_pages(db, t, now, |page, skip| {
        count += write_rows(&mut out, page, skip)?;
        Ok(true)
    })
    .await?;
    Ok((out, count))
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
