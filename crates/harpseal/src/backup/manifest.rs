//! `manifest.json` ([`Manifest`]) and the entry names it pins.

use serde::{Deserialize, Serialize};

use super::{BackupSource, FORMAT, MANIFEST, sha256_hex};
use crate::{Error, Result};

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
        if let Some(t) = m
            .tables
            .iter()
            .find(|t| !t.parts.is_empty() && !t.sha256.is_empty())
        {
            return Err(Error::Invalid(format!(
                "backup {MANIFEST}: table {} has both parts and a sha256 of its own",
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
    /// Its size in bytes.
    #[serde(default)]
    pub bytes: u64,
    /// The FileStore keys its rows' `file_key` / `*_file_key` columns
    /// name, sorted, each once (the objects are under `files`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
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
