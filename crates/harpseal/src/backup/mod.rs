//! Backup and restore of a whole Deployment through its ports: every table
//! via [`crate::Db`] and every object a row points at via
//! [`crate::FileStore`], so one format serves rusqlite and D1 alike.
//!
//! [`dump`] writes named entries to a [`BackupSink`]; [`load`] reads them
//! back from a [`BackupSource`]. The `harpseal backup` / `restore` commands
//! use a directory; tests use [`MemoryBackup`]. A backup (format 1) holds:
//!
//! - `manifest.json`: a [`Manifest`]: format, Deployment name, when it was
//!   taken, the applied migrations in order, each table's columns, row
//!   count and `CREATE` SQL, and each FileStore object's key, size and
//!   SHA-256.
//! - `tables/<table>.ndjson`: one JSON object per row, every column, in
//!   `rowid` order (primary-key order for `WITHOUT ROWID` tables). Values
//!   keep their SQLite type: `null`, integers as `42`, reals as `3.0`, text
//!   as strings, blobs as `{"$blob": "<base64>"}`.
//! - `files/<sha256 of the key>`: the bytes of each object named by a
//!   `file_key` or `*_file_key` column. An object missing from the
//!   FileStore is listed as `missing` and skipped, never fatal.
//!
//! Every user table is dumped except the `migration` table (a restore
//! re-runs the migrations) and SQLite's and D1's internal tables.
//!
//! **Consistency.** The `Db` port has no read transaction, so a dump reads
//! table by table while the Deployment may still be writing. Stop the
//! server (or at least its Job ticker) for the backup; the manifest records
//! when the dump started and finished.
//!
//! **Secrets.** Password hashes, session ids and Channel tokens stored in
//! the database are dumped as they are: restored logins keep working, and
//! the backup is as sensitive as the database itself.
//!
//! **Restore.** [`load`] refuses unless every table is empty, or
//! [`LoadOptions::force`] is set; it checks that the backup's migrations
//! are a prefix of the Deployment's, applies the Deployment's migrations,
//! checks the columns still fit, wipes (when forced) and inserts the rows
//! parents-first, puts the objects back, and writes the audit row
//! `backup.restore`. See [`load`] for the details.

mod dump;
mod load;
mod schema;
mod value;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::audit::RequestCtx;
use crate::{Error, Result};

pub use dump::dump;
pub use load::load;

/// The backup format this build writes and reads.
pub const FORMAT: u32 = 1;
/// Name of the manifest entry.
pub const MANIFEST: &str = "manifest.json";
/// Rows read per query, and statements per insert batch.
pub const PAGE: usize = 500;

/// Where [`dump`] writes its entries (a directory, memory, an archive).
pub trait BackupSink {
    /// Stores entry `name` (`manifest.json`, `tables/<t>.ndjson`,
    /// `files/<hex>`). Each name is written once.
    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<()>;
}

/// Where [`load`] reads the entries [`dump`] wrote.
pub trait BackupSource {
    /// The bytes of entry `name`; an error when it is missing.
    fn read(&self, name: &str) -> Result<Vec<u8>>;
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

impl BackupSink for MemoryBackup {
    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        self.entries.insert(name.to_string(), bytes.to_vec());
        Ok(())
    }
}

impl BackupSource for MemoryBackup {
    fn read(&self, name: &str) -> Result<Vec<u8>> {
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
    pub fn read(source: &dyn BackupSource) -> Result<(Manifest, Vec<u8>)> {
        let raw = source.read(MANIFEST)?;
        let m: Manifest = serde_json::from_slice(&raw)
            .map_err(|e| Error::Invalid(format!("backup {MANIFEST}: {e}")))?;
        if m.format != FORMAT {
            return Err(Error::Invalid(format!(
                "backup format {} is not supported (this build reads format {FORMAT})",
                m.format
            )));
        }
        Ok((m, raw))
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
    /// Table name; its rows are in `tables/<name>.ndjson`.
    pub name: String,
    /// Rows dumped.
    pub rows: u64,
    /// Every column, in table order.
    pub columns: Vec<String>,
    /// `sqlite_master.sql` when the dump was taken.
    pub sql: String,
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
}

/// How [`load`] runs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LoadOptions {
    /// Restore into a database that has rows: they are deleted first.
    pub force: bool,
    /// Context of the `backup.restore` audit row ([`RequestCtx::cli`]).
    pub ctx: RequestCtx,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            force: false,
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
    /// Whether the database had rows that were deleted first.
    pub wiped: bool,
    /// `force` as given.
    pub forced: bool,
    /// Tables of the Deployment absent from the backup: left as they were.
    pub untouched: Vec<String>,
    /// Migrations this restore applied before inserting.
    pub migrations_applied: usize,
    /// SHA-256 of `manifest.json`, also in the audit row.
    pub manifest_sha256: String,
}

/// Lowercase hex SHA-256.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    crate::id::hex(&Sha256::digest(bytes))
}

/// Entry name of a table's rows.
pub(crate) fn table_entry(table: &str) -> String {
    format!("tables/{table}.ndjson")
}

/// Entry name of the object stored under `key`.
pub(crate) fn file_entry(key: &str) -> String {
    format!("files/{}", sha256_hex(key.as_bytes()))
}
