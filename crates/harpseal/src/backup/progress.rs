//! [`DumpProgress`]: [`super::dump`] in steps, so a Job can spread one
//! backup over several runs (a Workers invocation has CPU, memory and
//! subrequest limits) and resume after a crash. The state is plain JSON,
//! saved by the caller between steps.
//!
//! A dump has two phases: every table, children before parents, each in
//! parts (`parts.rs`), then every FileStore object the rows name.
//! [`Budget`] bounds one step; a step that runs out in the middle of a
//! table ends the part it holds and saves the key it stopped after (the
//! next step goes on from there), so a step holds at most one part in
//! memory, however large the table.

use std::collections::HashSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::dump::applied_migrations;
use super::parts::{Cursor, sidecar};
use super::schema::{self, Table};
use super::{
    BackupSink, DumpReport, FORMAT, MANIFEST, Manifest, ManifestFile, ManifestMigration,
    ManifestPart, ManifestTable, file_entry, part_entry, sha256_hex,
};
use crate::config::BackupConfig;
use crate::{Deployment, Error, Result};

/// Where a dump stands: what is written so far, and what is left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DumpProgress {
    /// When the dump started, ms UTC (`taken_at`).
    pub(crate) taken_at: i64,
    pub(crate) migrations: Vec<ManifestMigration>,
    /// Every table to dump, children first (see [`DumpProgress::start`]).
    pub(crate) order: Vec<String>,
    /// Those tables as the database described them at the start, so a
    /// step does not read the schema again (two `PRAGMA`s per table).
    #[serde(default)]
    pub(crate) schema: Vec<Table>,
    /// Tables written so far, in `order`.
    pub(crate) tables: Vec<ManifestTable>,
    /// The table being written (the next in `order`), when a step stopped
    /// inside it: its parts so far and the key they end at.
    #[serde(default)]
    pub(crate) cursor: Option<Cursor>,
    /// FileStore keys named by tables written before table parts (a run
    /// started by an older build); now each part lists its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) keys: Vec<String>,
    /// Objects copied so far, in [`DumpProgress::object_keys`] order.
    pub(crate) files: Vec<ManifestFile>,
    /// Saved key hashes (`settle.rs`) of tables finished, to delete.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) stale: Vec<String>,
    /// Bytes written so far (entries, not counting the manifest).
    pub(crate) bytes: u64,
}

/// How much one [`DumpProgress::step`] may do before it stops, and how
/// large a part may grow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Budget {
    /// Rows dumped; checked before each page.
    pub(crate) rows: u64,
    /// Bytes written (and held for the part being built); checked before
    /// each page and object.
    pub(crate) bytes: u64,
    /// Objects copied; checked before each object.
    pub(crate) files: u64,
    /// Subrequests (database queries and FileStore calls) one step may
    /// make; a page or object that would go past it waits for the next
    /// step, unless the step has done nothing yet.
    pub(crate) queries: u64,
    /// A part ends at this many rows (`[backup] part_rows`).
    pub(crate) part_rows: u64,
    /// A part ends once it holds this many bytes (`[backup] part_mb`); it
    /// may pass it by one page of rows.
    pub(crate) part_bytes: u64,
    /// Stop once the last table is written, so the caller saves the keys
    /// before any object is copied: every entry the dump may write is then
    /// named by saved progress (and can be deleted if the backup fails).
    pub(crate) pause_before_files: bool,
}

impl Budget {
    /// No step limit: the whole dump in one step, in parts as `[backup]`
    /// sizes them (so a native backup and the nightly Job split a table
    /// alike).
    pub(crate) fn unlimited(c: &BackupConfig) -> Budget {
        Budget {
            rows: u64::MAX,
            bytes: u64::MAX,
            files: u64::MAX,
            queries: u64::MAX,
            part_rows: c.part_rows,
            part_bytes: u64::from(c.part_mb) * 1024 * 1024,
            pause_before_files: false,
        }
    }
}

/// What one step has done so far, against its [`Budget`].
#[derive(Debug, Default)]
pub(super) struct Spent {
    pub(super) rows: u64,
    pub(super) bytes: u64,
    pub(super) files: u64,
    /// Subrequests made.
    pub(super) used: u64,
    /// Whether the step has read or copied anything (it always does once).
    pub(super) worked: bool,
}

impl Spent {
    /// Whether the rows, bytes (with `held` not yet written) or objects
    /// are used up.
    pub(super) fn over(&self, b: &Budget, held: u64) -> bool {
        self.rows >= b.rows || self.bytes + held >= b.bytes || self.files >= b.files
    }
}

/// Where a run keeps its state between steps: the progress, saved in the
/// middle of a step before it writes an entry the saved progress does not
/// name yet (so an abandoned backup can always be deleted entry by entry,
/// [`DumpProgress::entries`]), and the key hashes a table's check needs
/// in a later step (`settle.rs`), outside the backup.
#[async_trait]
pub(crate) trait Checkpoint: Send + Sync {
    /// Stores `p` where the next step will read it.
    async fn save(&self, p: &DumpProgress) -> Result<()>;
    /// Stores the key hashes `name` ([`super::parts::sidecar`]).
    async fn put_check(&self, name: &str, bytes: Vec<u8>) -> Result<()>;
    /// The key hashes `name`, if stored.
    async fn get_check(&self, name: &str) -> Result<Option<Vec<u8>>>;
    /// Deletes the key hashes `name` (missing is not an error).
    async fn delete_check(&self, name: &str) -> Result<()>;
}

/// A dump in one step (`harpseal backup`, tests): nothing to save between
/// steps, so nothing is kept.
pub(crate) struct Local;

#[async_trait]
impl Checkpoint for Local {
    async fn save(&self, _: &DumpProgress) -> Result<()> {
        Ok(())
    }
    async fn put_check(&self, _: &str, _: Vec<u8>) -> Result<()> {
        Ok(())
    }
    async fn get_check(&self, _: &str) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    async fn delete_check(&self, _: &str) -> Result<()> {
        Ok(())
    }
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
        let schema = order
            .iter()
            .filter_map(|n| tables.iter().find(|t| &t.name == n).cloned())
            .collect();
        Ok(Self {
            taken_at,
            migrations,
            order,
            schema,
            tables: Vec::new(),
            cursor: None,
            keys: Vec::new(),
            files: Vec::new(),
            stale: Vec::new(),
            bytes: 0,
        })
    }

    /// Every entry name this dump has written or may still write next:
    /// the parts of each table written, the parts so far and the next one
    /// of the table being written, the first part of each table left, each
    /// named object's and the manifest. What to delete when a backup is
    /// abandoned.
    pub(crate) fn entries(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tables.iter().flat_map(|t| t.entries()).collect();
        if let Some(c) = &self.cursor {
            names.extend(c.parts.iter().map(|p| p.entry.clone()));
        }
        names.extend(self.next_entry());
        let left = self.order.iter().skip(self.tables.len() + 1);
        names.extend(left.map(|t| part_entry(t, 0)));
        names.extend(self.object_keys().iter().map(|k| file_entry(k)));
        names.push(MANIFEST.to_string());
        names
    }

    /// The saved key hashes this run may have left (`settle.rs`): those of
    /// finished tables not yet deleted, and the table being written's.
    pub(crate) fn checks(&self) -> Vec<String> {
        let mut names = self.stale.clone();
        if let (Some(c), Some(t)) = (&self.cursor, self.order.get(self.tables.len())) {
            names.extend((0..=c.segments).map(|k| sidecar(t, k)));
        }
        names
    }

    /// Every FileStore key the written rows name, each once, in table and
    /// part order: the objects to copy.
    pub(crate) fn object_keys(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut parts: Vec<&ManifestPart> = self.tables.iter().flat_map(|t| &t.parts).collect();
        if let Some(c) = &self.cursor {
            parts.extend(&c.parts);
        }
        let named = parts.into_iter().flat_map(|p| &p.files);
        self.keys
            .iter()
            .chain(named)
            .filter(|k| seen.insert(k.as_str()))
            .cloned()
            .collect()
    }

    /// Deletes the saved key hashes of finished tables.
    async fn sweep(&mut self, checkpoint: &dyn Checkpoint, spent: &mut Spent) -> Result<()> {
        for name in std::mem::take(&mut self.stale) {
            checkpoint.delete_check(&name).await?;
            spent.used += 1;
        }
        Ok(())
    }

    /// The part the dump writes next, if a table is left.
    pub(super) fn next_entry(&self) -> Option<String> {
        let name = self.order.get(self.tables.len())?;
        let n = self.cursor.as_ref().map_or(0, |c| c.parts.len());
        Some(part_entry(name, n))
    }

    /// Dumps tables, then objects, until `budget` runs out or the backup
    /// is complete. Idempotent per entry: a step re-run after a crash
    /// rewrites the same entries. Saves the progress through `checkpoint`
    /// before writing a part it does not name, and the key hashes a later
    /// step needs (see [`Checkpoint`]).
    pub(crate) async fn step(
        &mut self,
        d: &Deployment,
        sink: &mut dyn BackupSink,
        budget: Budget,
        checkpoint: &dyn Checkpoint,
    ) -> Result<Step> {
        let s = d.services();
        let db = s.db.as_ref();
        let mut spent = Spent::default();
        self.sweep(checkpoint, &mut spent).await?;
        // The part the saved progress names as next.
        let mut named = self.next_entry();
        let tables_before = self.tables.len();
        if self.tables.len() < self.order.len() {
            let current = if self.schema.is_empty() {
                schema::tables(db).await?
            } else {
                self.schema.clone()
            };
            while let Some(name) = self.order.get(self.tables.len()).cloned() {
                if name.contains('/') {
                    return Err(Error::Invalid(format!(
                        "table {name}: a table name with `/` cannot be backed up (its parts \
                         would be named like another table's)"
                    )));
                }
                let t = current.iter().find(|t| t.name == name).ok_or_else(|| {
                    Error::Invalid(format!("table {name} disappeared during the backup"))
                })?;
                let mut w = super::parts::Writer {
                    progress: self,
                    sink: &mut *sink,
                    budget: &budget,
                    spent: &mut spent,
                    checkpoint,
                    named: &mut named,
                };
                if !w.table(db, t, s.now_ms()).await? {
                    return Ok(Step::More);
                }
            }
        }
        if budget.pause_before_files && self.tables.len() > tables_before {
            return Ok(Step::More);
        }
        let keys = self.object_keys();
        while let Some(key) = keys.get(self.files.len()).cloned() {
            // A copy is a read and a write.
            let room = spent.used + 2 <= budget.queries;
            if spent.worked && (spent.over(&budget, 0) || !room) {
                return Ok(Step::More);
            }
            spent.used += 2;
            spent.worked = true;
            let entry = file_entry(&key);
            let found = s.files.get(&key).await?;
            let file = match found {
                Some(object) => {
                    sink.write(&entry, &object).await?;
                    spent.bytes += object.len() as u64;
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
            spent.files += 1;
        }
        self.sweep(checkpoint, &mut spent).await?;
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
