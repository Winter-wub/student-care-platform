//! [`load`]: a backup from a [`BackupSource`] into a Deployment.

use std::collections::BTreeMap;

use serde_json::json;

use super::check::{backed_migrations, check_columns, decode_table};
use super::schema::{self, Table, quote};
use super::{
    BackupSource, LoadOptions, LoadReport, Manifest, ManifestFile, PAGE, file_entry, sha256_hex,
    table_entry,
};
use crate::audit::{self, Actor};
use crate::port_types::{Statement, Value};
use crate::{Deployment, Error, Result};

/// The session tables `drop_sessions` empties.
const SESSION_TABLES: [&str; 2] = ["staff_session", "app_session"];

/// Restores the backup in `source` into `d`.
///
/// **Checks, before any row is written** (a refusal leaves the data as it
/// was; step 2 may create tables in an empty database):
///
/// 1. reads `manifest.json` (format 1 only) and checks each FileStore key;
/// 2. checks, per Module, that the backup's migrations are the first of
///    that Module's here (same names and checksums, in order), and applies
///    just those ([`crate::migrate`], without the `[roles]` seed: the Roles
///    come from the backup), so the schema is the backup's;
/// 3. checks every table of the backup exists with all its columns, that
///    any column the backup lacks is nullable or has a default (possible
///    only when the database was already ahead of the backup), and that no
///    table of the backup has an `INSERT` trigger (refused: the inserts
///    would set it off once per row);
/// 4. reads every table entry and FileStore object, checking each against
///    its SHA-256 in the manifest and the row counts; the decoded rows stay
///    in memory until they are inserted;
/// 5. refuses when any table has rows, unless `opts.force`.
///
/// **Writes:**
///
/// 6. puts each object back under its key (`application/octet-stream`:
///    the FileStore port reports no content types); objects go first
///    since putting one again is harmless;
/// 7. with `opts.force`, in one `Db::batch`: drops the triggers on the
///    backup's tables that guard `DELETE` (the append-only `audit_log`,
///    `security_event`, `consent`), deletes every row of every table in the
///    backup (children first) and re-creates those triggers from their
///    saved SQL. Tables absent from the backup, and the schema, are left
///    as they are. The counts deleted and the newest deleted audit row go
///    into the restore's audit row;
/// 8. inserts the rows parents first (foreign keys hold on rusqlite) in
///    batches of [`PAGE`] statements. The last batch also writes the audit
///    row `backup.restore` (actor System, `after` = `{manifest_sha256,
///    tables, rows, files, forced, deleted, newest_deleted_audit,
///    dropped_sessions, menu_salt_redrawn}`), with `opts.drop_sessions`
///    empties `staff_session` and `app_session`, and draws a new Menu
///    revision salt (`menu_salt`), so a restored copy on the same LINE
///    token never names its rich menus like the original;
/// 9. applies the Deployment's newer migrations, so data migrations run
///    over the restored rows.
///
/// Steps 7 to 9 are not one transaction: if the inserts fail part-way the
/// database holds part of the backup (and no audit row); run it again with
/// `force`.
pub async fn load(
    d: &Deployment,
    source: &dyn BackupSource,
    opts: &LoadOptions,
) -> Result<LoadReport> {
    let s = d.services();
    let db = s.db.as_ref();
    let (manifest, raw) = Manifest::read(source).await?;
    let manifest_sha256 = sha256_hex(&raw);
    for f in &manifest.files {
        crate::check_file_key(&f.key)?;
        if f.entry != file_entry(&f.key) {
            return Err(Error::Invalid(format!(
                "backup object {} has entry {}, expected {}",
                f.key,
                f.entry,
                file_entry(&f.key)
            )));
        }
    }
    let backed = backed_migrations(&manifest.migrations, d.migrations())?;
    let first = crate::migrate::migrate(db, &backed, s.now_ms()).await?;

    let current = schema::tables(db).await?;
    for mt in &manifest.tables {
        check_columns(mt, &current)?;
    }
    let names: Vec<String> = manifest.tables.iter().map(|t| t.name.clone()).collect();
    let guarded = schema::insert_triggers(db, &names).await?;
    if !guarded.is_empty() {
        return Err(Error::Invalid(format!(
            "triggers fire on INSERT into tables of the backup ({}); a restore would set them \
             off for every row, so it is refused",
            guarded.join(", ")
        )));
    }
    let mut decoded: BTreeMap<String, Vec<Vec<Value>>> = BTreeMap::new();
    for mt in &manifest.tables {
        let bytes = source.read(&table_entry(&mt.name)).await?;
        decoded.insert(mt.name.clone(), decode_table(mt, &bytes)?);
    }
    for f in manifest.files.iter().filter(|f| !f.missing) {
        verified_object(source, f).await?;
    }

    let mut non_empty = Vec::new();
    let mut deleted = BTreeMap::new();
    for t in &current {
        let n = schema::count(db, &t.name).await?;
        if n > 0 {
            non_empty.push(format!("{} ({n} rows)", t.name));
            if names.contains(&t.name) {
                deleted.insert(t.name.clone(), n);
            }
        }
    }
    if !non_empty.is_empty() && !opts.force {
        return Err(Error::Conflict(format!(
            "the database is not empty: {}; restore into a fresh database, or force it to \
             delete the rows of every table in the backup first",
            non_empty.join(", ")
        )));
    }

    let mut files = 0usize;
    let mut missing_files = Vec::new();
    for f in &manifest.files {
        if f.missing {
            missing_files.push(f.key.clone());
            continue;
        }
        let bytes = verified_object(source, f).await?;
        s.files
            .put(&f.key, bytes, "application/octet-stream")
            .await?;
        files += 1;
    }

    // Parents first, over the backup's tables as the schema now has them.
    let in_backup: Vec<Table> = current
        .iter()
        .filter(|t| names.contains(&t.name))
        .cloned()
        .collect();
    let order = schema::dependency_order(&in_backup);
    let newest_deleted_audit = if opts.force && !deleted.is_empty() {
        let newest = newest_audit(d).await?;
        wipe(d, &order).await?;
        newest
    } else {
        deleted.clear();
        None
    };

    let mut stmts = Vec::new();
    let mut tables = Vec::with_capacity(order.len());
    let mut rows = 0u64;
    for name in &order {
        let mt = manifest
            .tables
            .iter()
            .find(|m| &m.name == name)
            .expect("ordered from the manifest");
        let values = decoded.remove(name).unwrap_or_default();
        let cols: Vec<String> = mt.columns.iter().map(|c| quote(c)).collect();
        let marks = vec!["?"; cols.len()].join(", ");
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({marks})",
            quote(name),
            cols.join(", ")
        );
        let n = values.len() as u64;
        stmts.extend(values.into_iter().map(|params| Statement {
            sql: sql.clone().into(),
            params,
        }));
        rows += n;
        tables.push((name.clone(), n));
    }

    // A restored copy may run next to its original on one LINE token: it
    // gets its own Menu revision salt (#66).
    let redraw_salt = current.iter().any(|t| t.name == "menu_salt");
    let drop_sessions: Vec<&str> = SESSION_TABLES
        .into_iter()
        .filter(|t| opts.drop_sessions && current.iter().any(|c| c.name == *t))
        .collect();
    let after = json!({
        "manifest_sha256": manifest_sha256,
        "tables": tables.len(),
        "rows": rows,
        "files": files,
        "forced": opts.force,
        "deleted": deleted,
        "newest_deleted_audit": newest_deleted_audit,
        "dropped_sessions": !drop_sessions.is_empty(),
        "menu_salt_redrawn": redraw_salt,
    });
    let mut tail: Vec<Statement> = drop_sessions
        .iter()
        .map(|t| Statement::new(format!("DELETE FROM {}", quote(t))))
        .collect();
    if redraw_salt {
        tail.push(crate::kernel::menu::redraw_salt(s)?);
    }
    tail.push(audit::entry(
        s,
        &Actor::System,
        "backup.restore",
        None,
        None,
        Some(after),
        &opts.ctx,
    )?);
    let mut batches: Vec<Vec<Statement>> = Vec::new();
    let mut it = stmts.into_iter().peekable();
    while it.peek().is_some() {
        batches.push(it.by_ref().take(PAGE).collect());
    }
    match batches.last_mut() {
        Some(last) => last.extend(tail),
        None => batches.push(tail),
    }
    let total = batches.len();
    for (i, batch) in batches.into_iter().enumerate() {
        db.batch(batch)
            .await
            .map_err(|e| Error::Db(format!("restoring rows, batch {} of {total}: {e}", i + 1)))?;
    }

    let rest = crate::migrate::migrate(db, d.migrations(), s.now_ms()).await?;
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
        wiped: !deleted.is_empty(),
        deleted,
        dropped_sessions: !drop_sessions.is_empty(),
        forced: opts.force,
        untouched,
        migrations_applied: first.applied.len() + rest.applied.len(),
        manifest_sha256,
    })
}

/// The bytes of a listed object, checked against its SHA-256.
async fn verified_object(source: &dyn BackupSource, f: &ManifestFile) -> Result<Vec<u8>> {
    let bytes = source.read(&f.entry).await?;
    if f.sha256.as_deref() != Some(sha256_hex(&bytes).as_str()) {
        return Err(Error::Invalid(format!(
            "backup object {} ({}) does not match its SHA-256",
            f.key, f.entry
        )));
    }
    Ok(bytes)
}

/// `{id, at}` of the newest `audit_log` row, if there is one.
async fn newest_audit(d: &Deployment) -> Result<Option<serde_json::Value>> {
    let rows = d
        .services()
        .db
        .query(Statement::new(
            "SELECT id, at FROM audit_log ORDER BY at DESC, id DESC LIMIT 1",
        ))
        .await?;
    Ok((!rows.is_empty()).then(|| {
        json!({
            "id": rows.get(0, "id").and_then(Value::as_text),
            "at": rows.get(0, "at").and_then(Value::as_i64),
        })
    }))
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
