//! [`FileStoreBackup`]: a backup kept as FileStore objects under one key
//! prefix (R2, S3, a local directory), as the nightly Job writes it.

use std::sync::Arc;

use async_trait::async_trait;

use super::{BackupSink, BackupSource};
use crate::{Error, FileStore, Result};

/// Entries stored as FileStore objects `<prefix><name>`: both a sink and
/// a source. On a FileStore that is a local directory the prefix is a
/// plain directory that `harpseal restore` reads as it is.
#[derive(Clone)]
pub struct FileStoreBackup {
    files: Arc<dyn FileStore>,
    prefix: String,
}

impl core::fmt::Debug for FileStoreBackup {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FileStoreBackup")
            .field("prefix", &self.prefix)
            .finish()
    }
}

impl FileStoreBackup {
    /// Entries under `prefix`, which should end in `/` (`backups/x/`).
    pub fn new(files: Arc<dyn FileStore>, prefix: impl Into<String>) -> Self {
        Self {
            files,
            prefix: prefix.into(),
        }
    }

    /// The FileStore key of entry `name`.
    pub fn key(&self, name: &str) -> String {
        format!("{}{name}", self.prefix)
    }

    /// Deletes the entries `names` (a missing one is not an error).
    pub async fn delete(&self, names: &[String]) -> Result<()> {
        for name in names {
            self.files.delete(&self.key(name)).await?;
        }
        Ok(())
    }
}

/// The content type an entry is stored with.
fn content_type(name: &str) -> &'static str {
    if name.ends_with(".ndjson") {
        "application/x-ndjson"
    } else if name.ends_with(".json") {
        "application/json"
    } else {
        "application/octet-stream"
    }
}

#[async_trait]
impl BackupSink for FileStoreBackup {
    async fn write(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        self.files
            .put(&self.key(name), bytes.to_vec(), content_type(name))
            .await
    }
}

#[async_trait]
impl BackupSource for FileStoreBackup {
    async fn read(&self, name: &str) -> Result<Vec<u8>> {
        self.files
            .get(&self.key(name))
            .await?
            .ok_or_else(|| Error::Invalid(format!("backup entry {name} is missing")))
    }
}
