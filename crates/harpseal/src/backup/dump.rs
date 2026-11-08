//! [`dump`]: the database and the FileStore objects it references, to a
//! [`BackupSink`].

use super::progress::{Budget, DumpProgress, Step};
use super::schema::{Table, quote};
use super::{BackupSink, DumpReport, ManifestMigration};
use crate::port_types::{Statement, Value};
use crate::{Db, Deployment, Error, Result};

/// Writes a backup of `d` to `sink`: each table's parts (children before
/// parents, see the module docs), one `files/<hex>` per referenced
/// FileStore object, then `manifest.json` last (a sink without a manifest
/// holds no finished backup). Reads in pages of [`super::PAGE`] rows, by
/// primary key; one part at a time is built in memory, sized by
/// `[backup] part_rows` and `part_mb`. Not one transaction: see the module
/// docs. The nightly backup Job runs the same dump in budgeted steps.
pub async fn dump(d: &Deployment, sink: &mut dyn BackupSink) -> Result<DumpReport> {
    let mut progress = DumpProgress::start(d).await?;
    let budget = Budget::unlimited(&d.config().backup);
    match progress.step(d, sink, budget, None).await? {
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
