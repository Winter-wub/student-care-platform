//! [`dump`]: the database and the FileStore objects it references, to a
//! [`BackupSink`].

use std::collections::BTreeSet;

use serde_json::{Map, Value as Json};

use super::schema::{self, Table, quote};
use super::value::encode;
use super::{
    BackupSink, DumpReport, FORMAT, MANIFEST, Manifest, ManifestFile, ManifestMigration,
    ManifestTable, PAGE, file_entry, sha256_hex, table_entry,
};
use crate::port_types::{Rows, Statement, Value};
use crate::{Db, Deployment, Error, Result};

/// Writes a backup of `d` to `sink`: one `tables/<t>.ndjson` per table,
/// one `files/<hex>` per referenced FileStore object, then
/// `manifest.json` last (a sink without a manifest holds no finished
/// backup). Reads in pages of [`PAGE`] rows; each table's entry is built in
/// memory before it is written. Not one transaction: see the module docs.
pub async fn dump(d: &Deployment, sink: &mut dyn BackupSink) -> Result<DumpReport> {
    let s = d.services();
    let db = s.db.as_ref();
    let taken_at = s.now_ms();
    let migrations = applied_migrations(db).await?;
    let tables = schema::tables(db).await?;
    let mut manifest_tables = Vec::with_capacity(tables.len());
    let mut keys = BTreeSet::new();
    for t in &tables {
        let (bytes, rows) = dump_table(db, t).await?;
        sink.write(&table_entry(&t.name), &bytes).await?;
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
        manifest_tables.push(ManifestTable {
            name: t.name.clone(),
            rows,
            sha256: sha256_hex(&bytes),
            columns: t.columns.iter().map(|c| c.name.clone()).collect(),
            sql: t.sql.clone(),
        });
    }
    let mut files = Vec::with_capacity(keys.len());
    let mut missing_files = Vec::new();
    for key in keys {
        let entry = file_entry(&key);
        match s.files.get(&key).await? {
            Some(bytes) => {
                sink.write(&entry, &bytes).await?;
                files.push(ManifestFile {
                    key,
                    entry,
                    bytes: Some(bytes.len() as u64),
                    sha256: Some(sha256_hex(&bytes)),
                    missing: false,
                });
            }
            None => {
                missing_files.push(key.clone());
                files.push(ManifestFile {
                    key,
                    entry,
                    bytes: None,
                    sha256: None,
                    missing: true,
                });
            }
        }
    }
    let manifest = Manifest {
        format: FORMAT,
        deployment: d.config().name.clone(),
        taken_at,
        finished_at: s.now_ms(),
        migrations,
        tables: manifest_tables,
        files,
    };
    let raw = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| Error::Invalid(format!("backup manifest: {e}")))?;
    sink.write(MANIFEST, &raw).await?;
    Ok(DumpReport {
        manifest,
        manifest_sha256: sha256_hex(&raw),
        missing_files,
    })
}

/// The `migration` table in applied (insertion) order.
async fn applied_migrations(db: &dyn Db) -> Result<Vec<ManifestMigration>> {
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
async fn dump_table(db: &dyn Db, t: &Table) -> Result<(Vec<u8>, u64)> {
    let mut out = Vec::new();
    let mut count = 0u64;
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
            count += write_rows(&mut out, &page, 0)?;
            if page.len() < PAGE {
                break;
            }
            offset += PAGE as i64;
        }
    } else {
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
            count += write_rows(&mut out, &page, 1)?;
            match page
                .rows
                .last()
                .and_then(|r| r.first())
                .and_then(Value::as_i64)
            {
                Some(last) if page.len() == PAGE => after = Some(last),
                _ => break,
            }
        }
    }
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
