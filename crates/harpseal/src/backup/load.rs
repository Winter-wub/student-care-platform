//! [`load`]: a backup from a [`BackupSource`] into a Deployment.

use serde_json::{Value as Json, json};

use super::schema::{self, Table, quote};
use super::value::decode;
use super::{
    BackupSource, LoadOptions, LoadReport, Manifest, ManifestMigration, ManifestTable, PAGE,
    sha256_hex, table_entry,
};
use crate::audit::{self, Actor};
use crate::port_types::{Statement, Value};
use crate::{Deployment, Error, Migration, Result};

/// Restores the backup in `source` into `d`. In order, refusing before any
/// write when a check fails:
///
/// 1. reads `manifest.json` (format 1 only);
/// 2. checks the backup's migrations are a prefix of the Deployment's
///    (same module, name and checksum, in order): a backup from another
///    Deployment or a newer build is refused;
/// 3. applies the Deployment's migrations ([`crate::migrate`], without the
///    `[roles]` seed: the Roles come from the backup), so the schema is
///    current;
/// 4. checks every table of the backup still exists with all its columns,
///    and that every column added since is nullable or has a default;
/// 5. reads every table and FileStore entry once, checking row counts,
///    values and SHA-256s;
/// 6. refuses when any table has rows, unless `opts.force`;
/// 7. with `opts.force`, in one `Db::batch`: drops the triggers on the
///    backup's tables that guard `DELETE` (the append-only `audit_log`,
///    `security_event`, `consent`), deletes every row of every table in the
///    backup (children first), and re-creates those triggers from their
///    saved SQL. Tables absent from the backup, and the schema, are left
///    as they are;
/// 8. inserts the rows parents first (foreign keys hold on rusqlite), in
///    batches of [`PAGE`] statements;
/// 9. puts each object back under its key (`application/octet-stream`:
///    the FileStore port does not report content types, so none is
///    stored);
/// 10. writes the audit row `backup.restore` (actor System, `after` =
///     `{manifest_sha256, tables, rows, files, forced}`).
///
/// Steps 8 and 9 are not one transaction: if they fail part-way the
/// database holds part of the backup; run it again with `force`.
pub async fn load(
    d: &Deployment,
    source: &dyn BackupSource,
    opts: &LoadOptions,
) -> Result<LoadReport> {
    let s = d.services();
    let db = s.db.as_ref();
    let (manifest, raw) = Manifest::read(source)?;
    let manifest_sha256 = sha256_hex(&raw);
    check_prefix(&manifest.migrations, d.migrations())?;
    let migrated = crate::migrate::migrate(db, d.migrations(), s.now_ms()).await?;
    let current = schema::tables(db).await?;
    for mt in &manifest.tables {
        check_columns(mt, &current)?;
    }
    for mt in &manifest.tables {
        read_table(source, mt)?;
    }
    for f in manifest.files.iter().filter(|f| !f.missing) {
        let bytes = source.read(&f.entry)?;
        if f.sha256.as_deref() != Some(sha256_hex(&bytes).as_str()) {
            return Err(Error::Invalid(format!(
                "backup object {} ({}) does not match its SHA-256",
                f.key, f.entry
            )));
        }
    }

    let mut non_empty = Vec::new();
    for t in &current {
        let n = schema::count(db, &t.name).await?;
        if n > 0 {
            non_empty.push(format!("{} ({n} rows)", t.name));
        }
    }
    if !non_empty.is_empty() && !opts.force {
        return Err(Error::Conflict(format!(
            "the database is not empty: {}; restore into a fresh database, or force it to \
             delete the rows of every table in the backup first",
            non_empty.join(", ")
        )));
    }

    // Parents first, over the backup's tables as the schema now has them.
    let in_backup: Vec<Table> = current
        .iter()
        .filter(|t| manifest.tables.iter().any(|m| m.name == t.name))
        .cloned()
        .collect();
    let order = schema::dependency_order(&in_backup);
    if opts.force {
        wipe(d, &order).await?;
    }

    let mut tables = Vec::with_capacity(order.len());
    let mut rows = 0u64;
    for name in &order {
        let mt = manifest
            .tables
            .iter()
            .find(|m| &m.name == name)
            .expect("ordered from the manifest");
        let values = read_table(source, mt)?;
        let cols: Vec<String> = mt.columns.iter().map(|c| quote(c)).collect();
        let marks = vec!["?"; cols.len()].join(", ");
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({marks})",
            quote(name),
            cols.join(", ")
        );
        let n = values.len() as u64;
        let mut it = values.into_iter().peekable();
        while it.peek().is_some() {
            let batch: Vec<Statement> = it
                .by_ref()
                .take(PAGE)
                .map(|params| Statement {
                    sql: sql.clone().into(),
                    params,
                })
                .collect();
            db.batch(batch)
                .await
                .map_err(|e| Error::Db(format!("restoring table {name}: {e}")))?;
        }
        rows += n;
        tables.push((name.clone(), n));
    }

    let mut files = 0usize;
    let mut missing_files = Vec::new();
    for f in &manifest.files {
        if f.missing {
            missing_files.push(f.key.clone());
            continue;
        }
        let bytes = source.read(&f.entry)?;
        s.files
            .put(&f.key, bytes, "application/octet-stream")
            .await?;
        files += 1;
    }

    let after = json!({
        "manifest_sha256": manifest_sha256,
        "tables": tables.len(),
        "rows": rows,
        "files": files,
        "forced": opts.force,
    });
    audit::record(
        s,
        &Actor::System,
        "backup.restore",
        None,
        None,
        Some(after),
        &opts.ctx,
    )
    .await?;

    let untouched = current
        .iter()
        .map(|t| t.name.clone())
        .filter(|n| !order.contains(n))
        .collect();
    Ok(LoadReport {
        tables,
        rows,
        files,
        missing_files,
        wiped: opts.force && !non_empty.is_empty(),
        forced: opts.force,
        untouched,
        migrations_applied: migrated.applied.len(),
        manifest_sha256,
    })
}

/// The backup's migrations must be the first of the Deployment's.
fn check_prefix(backup: &[ManifestMigration], ours: &[Migration]) -> Result<()> {
    for (i, b) in backup.iter().enumerate() {
        let Some(m) = ours.get(i) else {
            return Err(Error::Invalid(format!(
                "the backup has migration {}/{} that this Deployment does not have (it has {} \
                 migrations, the backup {}): restore it with the build that took it",
                b.module,
                b.name,
                ours.len(),
                backup.len()
            )));
        };
        let sum = crate::migrate::checksum(m.sql);
        if b.module != m.module || b.name != m.name || b.checksum != sum {
            return Err(Error::Invalid(format!(
                "migration #{} differs: the backup has {}/{} (checksum {}), this Deployment \
                 {}/{} (checksum {sum}): the backup comes from another Deployment or build",
                i + 1,
                b.module,
                b.name,
                b.checksum,
                m.module,
                m.name
            )));
        }
    }
    Ok(())
}

/// Every backup column must still exist; a new column must not need a value.
fn check_columns(mt: &ManifestTable, current: &[Table]) -> Result<()> {
    let Some(t) = current.iter().find(|t| t.name == mt.name) else {
        return Err(Error::Invalid(format!(
            "table {} is in the backup but not in this Deployment's schema",
            mt.name
        )));
    };
    if let Some(gone) = mt
        .columns
        .iter()
        .find(|c| !t.columns.iter().any(|tc| &tc.name == *c))
    {
        return Err(Error::Invalid(format!(
            "column {}.{gone} is in the backup but no longer in the schema",
            mt.name
        )));
    }
    if let Some(new) = t
        .columns
        .iter()
        .find(|c| c.not_null && !c.has_default && c.pk == 0 && !mt.columns.contains(&c.name))
    {
        return Err(Error::Invalid(format!(
            "column {}.{} is NOT NULL without a default and not in the backup",
            mt.name, new.name
        )));
    }
    Ok(())
}

/// The rows of `mt`, in manifest column order; checks the row count.
fn read_table(source: &dyn BackupSource, mt: &ManifestTable) -> Result<Vec<Vec<Value>>> {
    let entry = table_entry(&mt.name);
    let bytes = source.read(&entry)?;
    let bad = |line: usize, m: String| Error::Invalid(format!("backup {entry} line {line}: {m}"));
    let mut rows = Vec::with_capacity(mt.rows as usize);
    for (i, line) in bytes.split(|b| *b == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let obj: Json = serde_json::from_slice(line).map_err(|e| bad(i + 1, e.to_string()))?;
        let Json::Object(obj) = obj else {
            return Err(bad(i + 1, "not a JSON object".into()));
        };
        let row = mt
            .columns
            .iter()
            .map(|c| match obj.get(c) {
                Some(v) => decode(v).map_err(|e| bad(i + 1, e.to_string())),
                None => Err(bad(i + 1, format!("column {c} is missing"))),
            })
            .collect::<Result<Vec<Value>>>()?;
        rows.push(row);
    }
    if rows.len() as u64 != mt.rows {
        return Err(Error::Invalid(format!(
            "backup {entry} has {} rows, the manifest says {}",
            rows.len(),
            mt.rows
        )));
    }
    Ok(rows)
}

/// One batch: lift the `DELETE` guards, empty the tables children first,
/// put the guards back. Fails as a whole, leaving the triggers in place.
async fn wipe(d: &Deployment, order: &[String]) -> Result<()> {
    let db = d.services().db.as_ref();
    let triggers = schema::delete_triggers(db, order).await?;
    let mut batch = Vec::new();
    for (name, _) in &triggers {
        batch.push(Statement::new(format!("DROP TRIGGER {}", quote(name))));
    }
    for t in order.iter().rev() {
        batch.push(Statement::new(format!("DELETE FROM {}", quote(t))));
    }
    for (_, sql) in &triggers {
        batch.push(Statement::new(sql.clone()));
    }
    db.batch(batch).await.map_err(|e| {
        Error::Db(format!(
            "could not delete the existing rows (a table outside the backup may still \
             reference them): {e}"
        ))
    })?;
    Ok(())
}
