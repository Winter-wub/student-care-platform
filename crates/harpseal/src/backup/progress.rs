//! [`DumpProgress`]: [`super::dump`] in steps, so a Job can spread one
//! backup over several runs (a Workers invocation has CPU, memory and
//! subrequest limits) and resume after a crash. The state is plain JSON,
//! saved by the caller between steps.
//!
//! A dump has two phases: every table (one whole `tables/<t>.ndjson` entry
//! at a time, children before parents), then every FileStore object the
//! rows name. [`Budget`] bounds one step; a table is never split, so one
//! step dumps at least one table however large.

use serde::{Deserialize, Serialize};

use super::dump::{applied_migrations, dump_table, file_keys};
use super::schema;
use super::{
    BackupSink, DumpReport, FORMAT, MANIFEST, Manifest, ManifestFile, ManifestMigration,
    ManifestTable, file_entry, sha256_hex, table_entry,
};
use crate::{Deployment, Error, Result};

/// Where a dump stands: what is written so far, and what is left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DumpProgress {
    /// When the dump started, ms UTC (`taken_at`).
    pub(crate) taken_at: i64,
    pub(crate) migrations: Vec<ManifestMigration>,
    /// Every table to dump, children first (see [`DumpProgress::start`]).
    pub(crate) order: Vec<String>,
    /// Tables written so far, in `order`.
    pub(crate) tables: Vec<ManifestTable>,
    /// FileStore keys the written rows name, sorted, unique.
    pub(crate) keys: Vec<String>,
    /// Objects copied so far, in `keys` order.
    pub(crate) files: Vec<ManifestFile>,
    /// Bytes written so far (entries, not counting the manifest).
    pub(crate) bytes: u64,
}

/// How much one [`DumpProgress::step`] may do before it stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Budget {
    /// Rows dumped; checked before each table.
    pub(crate) rows: u64,
    /// Bytes written; checked before each table and object.
    pub(crate) bytes: u64,
    /// Objects copied; checked before each object.
    pub(crate) files: u64,
    /// Stop once the last table is written, so the caller saves the keys
    /// before any object is copied: every entry the dump may write is then
    /// named by saved progress (and can be deleted if the backup fails).
    pub(crate) pause_before_files: bool,
}

impl Budget {
    /// No limit: the whole dump in one step.
    pub(crate) const UNLIMITED: Budget = Budget {
        rows: u64::MAX,
        bytes: u64::MAX,
        files: u64::MAX,
        pause_before_files: false,
    };
}

/// What a step ended with.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Step {
    /// The budget ran out: save the progress and step again.
    More,
    /// `manifest.json` is written: the backup is complete.
    Done(DumpReport),
}

impl DumpProgress {
    /// A new dump of `d`: reads the applied migrations and the table list.
    ///
    /// **Order.** Tables are dumped children before parents (the reverse
    /// of the restore's insert order). The Kernel never deletes a row that
    /// others reference (Students, accounts, Staff, Tags and announcements
    /// are kept; a deleted Role takes its grants with it), so a child row
    /// read first still has its parent when the parent table is read
    /// later: a backup taken while the Deployment writes holds no dangling
    /// reference, and the restore's parents-first inserts succeed. Rows
    /// written during the dump may or may not be in it.
    pub(crate) async fn start(d: &Deployment) -> Result<Self> {
        let s = d.services();
        let db = s.db.as_ref();
        let taken_at = s.now_ms();
        let migrations = applied_migrations(db).await?;
        let tables = schema::tables(db).await?;
        let mut order = schema::dependency_order(&tables);
        order.reverse();
        Ok(Self {
            taken_at,
            migrations,
            order,
            tables: Vec::new(),
            keys: Vec::new(),
            files: Vec::new(),
            bytes: 0,
        })
    }

    /// Every entry name this dump has written or may still write: each
    /// table's, each named object's and the manifest. What to delete when
    /// a backup is abandoned.
    pub(crate) fn entries(&self) -> Vec<String> {
        let mut names: Vec<String> = self.order.iter().map(|t| table_entry(t)).collect();
        names.extend(self.keys.iter().map(|k| file_entry(k)));
        names.push(MANIFEST.to_string());
        names
    }

    /// Dumps tables, then objects, until `budget` runs out or the backup
    /// is complete. Idempotent per entry: a step re-run after a crash
    /// rewrites the same entries.
    pub(crate) async fn step(
        &mut self,
        d: &Deployment,
        sink: &mut dyn BackupSink,
        budget: Budget,
    ) -> Result<Step> {
        let s = d.services();
        let db = s.db.as_ref();
        let (mut rows, mut bytes, mut files) = (0u64, 0u64, 0u64);
        let spent = |rows: u64, bytes: u64, files: u64| {
            rows >= budget.rows || bytes >= budget.bytes || files >= budget.files
        };
        let tables_before = self.tables.len();
        if self.tables.len() < self.order.len() {
            let current = schema::tables(db).await?;
            while let Some(name) = self.order.get(self.tables.len()).cloned() {
                if spent(rows, bytes, files) {
                    return Ok(Step::More);
                }
                let t = current.iter().find(|t| t.name == name).ok_or_else(|| {
                    Error::Invalid(format!("table {name} disappeared during the backup"))
                })?;
                let (out, n) = dump_table(db, t).await?;
                sink.write(&table_entry(&t.name), &out).await?;
                for key in file_keys(db, t).await? {
                    if let Err(at) = self.keys.binary_search(&key) {
                        self.keys.insert(at, key);
                    }
                }
                self.tables.push(ManifestTable {
                    name: t.name.clone(),
                    rows: n,
                    sha256: sha256_hex(&out),
                    columns: t.columns.iter().map(|c| c.name.clone()).collect(),
                    sql: t.sql.clone(),
                });
                rows += n;
                bytes += out.len() as u64;
                self.bytes += out.len() as u64;
            }
        }
        if budget.pause_before_files && self.tables.len() > tables_before {
            return Ok(Step::More);
        }
        while let Some(key) = self.keys.get(self.files.len()).cloned() {
            if spent(rows, bytes, files) {
                return Ok(Step::More);
            }
            let entry = file_entry(&key);
            let found = s.files.get(&key).await?;
            let file = match found {
                Some(object) => {
                    sink.write(&entry, &object).await?;
                    bytes += object.len() as u64;
                    self.bytes += object.len() as u64;
                    ManifestFile {
                        key,
                        entry,
                        bytes: Some(object.len() as u64),
                        sha256: Some(sha256_hex(&object)),
                        missing: false,
                    }
                }
                None => ManifestFile {
                    key,
                    entry,
                    bytes: None,
                    sha256: None,
                    missing: true,
                },
            };
            self.files.push(file);
            files += 1;
        }
        self.finish(d, sink).await.map(Step::Done)
    }

    /// Writes `manifest.json` (last: a sink without it holds no finished
    /// backup).
    async fn finish(&self, d: &Deployment, sink: &mut dyn BackupSink) -> Result<DumpReport> {
        let manifest = Manifest {
            format: FORMAT,
            deployment: d.config().name.clone(),
            taken_at: self.taken_at,
            finished_at: d.services().now_ms(),
            migrations: self.migrations.clone(),
            tables: self.tables.clone(),
            files: self.files.clone(),
        };
        let raw = serde_json::to_vec_pretty(&manifest)
            .map_err(|e| Error::Invalid(format!("backup manifest: {e}")))?;
        sink.write(MANIFEST, &raw).await?;
        let missing_files = self
            .files
            .iter()
            .filter(|f| f.missing)
            .map(|f| f.key.clone())
            .collect();
        Ok(DumpReport {
            manifest,
            manifest_sha256: sha256_hex(&raw),
            missing_files,
            bytes: self.bytes + raw.len() as u64,
        })
    }
}
