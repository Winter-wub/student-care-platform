//! Backup and restore of a whole Deployment through its ports: every table
//! via [`crate::Db`] and every object a row points at via
//! [`crate::FileStore`], so one format serves rusqlite and D1 alike.
//!
//! [`dump`] writes named entries to a [`BackupSink`]; [`load`] reads them
//! back from a [`BackupSource`]. The `harpseal backup` / `restore` commands
//! use a directory; tests use [`MemoryBackup`]. A backup (format 2) holds:
//!
//! - `manifest.json`: a [`Manifest`]: format, Deployment name, when it was
//!   taken, the applied migrations in order, each table's columns, row
//!   count and `CREATE` SQL, and each FileStore object's key, size and
//!   SHA-256.
//!   Each table's entry is pinned by its SHA-256 too, so the manifest's
//!   own hash (recorded by the restore's audit row) pins the whole backup.
//! - each table's rows in one or more **parts** (#88), in order:
//!   `tables/<table>.ndjson`, then `tables/<table>/00001.ndjson`,
//!   `tables/<table>/00002.ndjson`, …; one JSON object per row, every
//!   column, in primary-key order (`rowid` order for a table without a
//!   primary key) across the parts. A part ends at `[backup] part_rows`
//!   rows or once it passes `part_mb`, or where a nightly Job step ended,
//!   so no step holds more than a part in memory. Each part is pinned by
//!   its SHA-256 and row count. Format 1 (before #88) is the same with
//!   one entry per table, pinned by `sha256` on the table; a restore reads
//!   both.
//!   Live Verification challenges are left out (see `pages.rs`). Values
//!   keep their SQLite type: `null`, integers as `42`, reals as `3.0`, text
//!   as strings, blobs as `{"$blob": "<base64>"}`.
//! - `files/<sha256 of the key>`: the bytes of each object named by a
//!   `file_key` or `*_file_key` column. An object missing from the
//!   FileStore is listed as `missing` and skipped, never fatal.
//!
//! Every user table is dumped except the `migration` table (a restore
//! re-runs the migrations) and SQLite's and D1's internal tables.
//!
//! **Consistency.** The `Db` port has no read transaction (D1 has none),
//! so a dump reads table by table while the Deployment may still be
//! writing; the nightly backup Job (#78) runs it in steps over minutes.
//! What holds anyway (ADR-0008, amended):
//!
//! - each table entry is pinned by its SHA-256 and row count, and the
//!   manifest records when the dump started and finished;
//! - a table is read by keyset on its primary key, so a row deleted and
//!   re-inserted under the same key mid-dump is read at most once (no
//!   duplicate keys);
//! - tables are read children before parents, and the Kernel deletes no
//!   referenced row except a Role (whose grants cascade), so a reference
//!   resolves unless its parent was deleted through a cascading key, and
//!   such orphans are dropped by the restore ([`load`] checks keys and
//!   references before it writes anything).
//!
//! What does not: the tables are not one point in time (a row written
//! mid-dump may or may not be in it). `harpseal backup` with the server
//! stopped is the one point-in-time copy.
//!
//! **Secrets.** Password hashes, session ids and Channel tokens stored in
//! the database are dumped as they are: restored logins keep working, and
//! the backup is as sensitive as the database itself. Live Verification
//! challenges (`pending`, not yet expired) are never dumped.
//!
//! **Restore.** [`load`] refuses unless every table is empty, or
//! [`LoadOptions::force`] is set; it checks that each Module's migrations
//! in the backup are a prefix of that Module's in the Deployment, applies
//! just those, verifies every entry, puts the objects back, wipes (when
//! forced) and inserts the rows parents-first, then applies the newer
//! migrations so they run over the restored rows. See [`load`].

mod check;
mod dump;
mod load;
mod pages;
mod parts;
pub(crate) mod progress;
pub(crate) mod schema;
mod store;
mod unique;
mod value;
mod verify;
pub(crate) mod xlsx;

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::audit::RequestCtx;
use crate::{Error, Result};

pub use dump::dump;
pub use load::load;
pub use store::FileStoreBackup;

/// The backup format this build writes. It reads this one and format 1
/// (one entry per table, no parts).
pub const FORMAT: u32 = 2;
/// Name of the manifest entry.
pub const MANIFEST: &str = "manifest.json";
/// Rows read per query, and statements per insert batch.
pub const PAGE: usize = 500;

/// Where [`dump`] writes its entries (a directory, memory, an object
/// store).
#[async_trait]
pub trait BackupSink: Send {
    /// Stores entry `name` (`manifest.json`, `tables/<t>.ndjson`,
    /// `files/<hex>`). Each name is written once.
    async fn write(&mut self, name: &str, bytes: &[u8]) -> Result<()>;
}

/// Where [`load`] reads the entries [`dump`] wrote.
#[async_trait]
pub trait BackupSource: Send + Sync {
    /// The bytes of entry `name`; an error when it is missing.
    async fn read(&self, name: &str) -> Result<Vec<u8>>;
}

/// A backup held in memory, by entry name: both a sink and a source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MemoryBackup {
    /// Entry name to bytes.
    pub entries: BTreeMap<String, Vec<u8>>,
}

impl MemoryBackup {
    /// An empty backup.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl BackupSink for MemoryBackup {
    async fn write(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        self.entries.insert(name.to_string(), bytes.to_vec());
        Ok(())
    }
}

#[async_trait]
impl BackupSource for MemoryBackup {
    async fn read(&self, name: &str) -> Result<Vec<u8>> {
        self.entries
            .get(name)
            .cloned()
            .ok_or_else(|| Error::Invalid(format!("backup entry {name} is missing")))
    }
}

/// `manifest.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Manifest {
    /// [`FORMAT`].
    pub format: u32,
    /// The Deployment's `name` from `deployment.toml`.
    pub deployment: String,
    /// When the dump started, ms UTC.
    pub taken_at: i64,
    /// When it finished, ms UTC.
    pub finished_at: i64,
    /// The `migration` table, in applied order.
    pub migrations: Vec<ManifestMigration>,
    /// Every table dumped, by name.
    pub tables: Vec<ManifestTable>,
    /// Every FileStore object referenced, by key.
    pub files: Vec<ManifestFile>,
}

impl Manifest {
    /// Reads and parses `manifest.json`; refuses another format.
    pub async fn read(source: &dyn BackupSource) -> Result<(Manifest, Vec<u8>)> {
        let raw = source.read(MANIFEST).await?;
        let m: Manifest = serde_json::from_slice(&raw)
            .map_err(|e| Error::Invalid(format!("backup {MANIFEST}: {e}")))?;
        if m.format != 1 && m.format != FORMAT {
            return Err(Error::Invalid(format!(
                "backup format {} is not supported (this build reads formats 1 and {FORMAT})",
                m.format
            )));
        }
        if m.format == 1
            && let Some(t) = m.tables.iter().find(|t| !t.parts.is_empty())
        {
            return Err(Error::Invalid(format!(
                "backup {MANIFEST}: table {} has parts, which format 1 does not have",
                t.name
            )));
        }
        Ok((m, raw))
    }

    /// Every entry the manifest names: itself, each table's parts and
    /// each object that was not missing.
    pub fn entries(&self) -> Vec<String> {
        let mut names = vec![MANIFEST.to_string()];
        names.extend(self.tables.iter().flat_map(ManifestTable::entries));
        names.extend(
            self.files
                .iter()
                .filter(|f| !f.missing)
                .map(|f| file_entry(&f.key)),
        );
        names
    }
}

/// One applied migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ManifestMigration {
    /// Owning Module (`kernel` for the Kernel).
    pub module: String,
    /// File name without extension.
    pub name: String,
    /// SHA-256 of its text, as in the `migration` table.
    pub checksum: String,
}

/// One dumped table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ManifestTable {
    /// Table name; its rows are in [`ManifestTable::parts`], or, without
    /// parts (format 1), in `tables/<name>.ndjson`.
    pub name: String,
    /// Rows dumped, in all its parts.
    pub rows: u64,
    /// Format 1: SHA-256 of the one entry's exact bytes. Empty (and not
    /// written) when the table has parts.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sha256: String,
    /// Format 2: the table's parts, in order; part `n` is entry
    /// [`part_entry`]`(name, n)`. Empty in format 1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<ManifestPart>,
    /// Every column, in table order.
    pub columns: Vec<String>,
    /// `sqlite_master.sql` when the dump was taken.
    pub sql: String,
}

impl ManifestTable {
    /// The entries holding its rows, in order.
    pub fn entries(&self) -> Vec<String> {
        if self.parts.is_empty() {
            vec![table_entry(&self.name)]
        } else {
            self.parts.iter().map(|p| p.entry.clone()).collect()
        }
    }
}

/// One part of a table (format 2): a run of its rows, in key order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ManifestPart {
    /// Entry name: [`part_entry`] of the table and the part's index.
    pub entry: String,
    /// Rows in it.
    pub rows: u64,
    /// SHA-256 of its exact bytes.
    pub sha256: String,
}

/// One FileStore object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ManifestFile {
    /// The FileStore key, as stored in the rows.
    pub key: String,
    /// Entry name of its bytes (`files/<sha256 of key>`).
    pub entry: String,
    /// Size, absent when missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// SHA-256 of the bytes, absent when missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// The FileStore had no object under this key when the dump ran.
    #[serde(default)]
    pub missing: bool,
}

/// What [`dump`] wrote.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DumpReport {
    /// The manifest written.
    pub manifest: Manifest,
    /// SHA-256 of `manifest.json` as written.
    pub manifest_sha256: String,
    /// Keys referenced by a row but absent from the FileStore.
    pub missing_files: Vec<String>,
    /// Bytes written, every entry and the manifest.
    pub bytes: u64,
}

/// How [`load`] runs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LoadOptions {
    /// Restore into a database that has rows: they are deleted first.
    pub force: bool,
    /// Delete every Staff and App Session after the inserts (in the same
    /// batch as the last rows): restored sessions stop working, and
    /// everyone signs in again. Advised after an incident.
    pub drop_sessions: bool,
    /// Context of the `backup.restore` audit row ([`RequestCtx::cli`]).
    pub ctx: RequestCtx,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            force: false,
            drop_sessions: false,
            ctx: RequestCtx::cli(),
        }
    }
}

impl LoadOptions {
    /// No force, the CLI's audit context.
    pub fn new() -> Self {
        Self::default()
    }
    /// Sets [`LoadOptions::force`].
    pub fn force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }
    /// Sets [`LoadOptions::drop_sessions`].
    pub fn drop_sessions(mut self, drop: bool) -> Self {
        self.drop_sessions = drop;
        self
    }
}

/// What [`load`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LoadReport {
    /// Rows inserted per table, in insert order.
    pub tables: Vec<(String, u64)>,
    /// Rows inserted in all.
    pub rows: u64,
    /// FileStore objects put back.
    pub files: usize,
    /// Keys the backup listed as missing (not restored).
    pub missing_files: Vec<String>,
    /// Whether the forced wipe deleted any row.
    pub wiped: bool,
    /// Rows the forced wipe deleted, per table (tables that had rows).
    pub deleted: BTreeMap<String, u64>,
    /// Whether the session tables were emptied.
    pub dropped_sessions: bool,
    /// Rows left out per table because their parent was deleted (through
    /// an `ON DELETE CASCADE` key) while the backup was being taken.
    pub orphans_dropped: BTreeMap<String, u64>,
    /// `force` as given.
    pub forced: bool,
    /// Tables of the Deployment absent from the backup: left as they were.
    pub untouched: Vec<String>,
    /// Migrations this restore applied: those the backup had, before the
    /// inserts, plus the newer ones after.
    pub migrations_applied: usize,
    /// SHA-256 of `manifest.json`, also in the audit row.
    pub manifest_sha256: String,
}

/// Lowercase hex SHA-256.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    crate::id::hex(&Sha256::digest(bytes))
}

/// Entry name of a table's rows in format 1, and of its first part.
pub(crate) fn table_entry(table: &str) -> String {
    format!("tables/{table}.ndjson")
}

/// Entry name of part `n` of `table` (from 0): the first is the format-1
/// name, so a table of one part is laid out as in format 1; the others
/// are `tables/<table>/<n, five digits>.ndjson`.
pub fn part_entry(table: &str, n: usize) -> String {
    if n == 0 {
        table_entry(table)
    } else {
        format!("tables/{table}/{n:05}.ndjson")
    }
}

/// Entry name of the object stored under `key`.
pub(crate) fn file_entry(key: &str) -> String {
    format!("files/{}", sha256_hex(key.as_bytes()))
}
