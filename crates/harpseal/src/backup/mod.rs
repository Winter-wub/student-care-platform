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
//!   `file_key` or `*_file_key` column, or by one of the three branding
//!   file settings (#114: the logo and Menu images; Locale `''`, and only a
//!   value of the shape the Kernel writes, so no other `setting` is ever
//!   followed as a file). An object missing from the FileStore is listed as `missing`
//!   and skipped, never fatal.
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
mod cursor;
mod dump;
mod hashes;
mod load;
mod manifest;
mod pages;
mod parts;
pub(crate) mod progress;
pub(crate) mod schema;
mod settle;
mod store;
mod unique;
pub(crate) mod value;
mod verify;
pub(crate) mod xlsx;

use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::audit::RequestCtx;
use crate::{Error, Result};

pub use dump::dump;
pub use load::load;
pub use manifest::{
    Manifest, ManifestFile, ManifestMigration, ManifestPart, ManifestTable, part_entry,
};
pub(crate) use manifest::{file_entry, table_entry};
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
