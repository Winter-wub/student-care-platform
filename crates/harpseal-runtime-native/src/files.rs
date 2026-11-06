//! [`LocalFiles`]: a directory on the server's disk as the `FileStore`.

use std::path::PathBuf;

use async_trait::async_trait;
use harpseal::{Error, FileStore, Random, Result, check_file_key};

use crate::OsRandom;

/// A local directory as the FileStore. Keys follow [`check_file_key`]
/// (the same rule as R2 and S3).
/// `content_type` is not stored (S3 and R2 keep it; a directory cannot).
#[derive(Debug, Clone)]
pub struct LocalFiles {
    root: PathBuf,
}

impl LocalFiles {
    /// Files live under `root`, created on the first `put`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, key: &str) -> Result<PathBuf> {
        check_file_key(key)?;
        Ok(self.root.join(key))
    }
}

pub(crate) fn io(e: std::io::Error) -> Error {
    Error::Platform(e.to_string())
}

#[async_trait]
impl FileStore for LocalFiles {
    async fn put(&self, key: &str, bytes: Vec<u8>, _content_type: &str) -> Result<()> {
        let p = self.path(key)?;
        if let Some(dir) = p.parent() {
            tokio::fs::create_dir_all(dir).await.map_err(io)?;
        }
        // Write a uniquely named sibling temp file, then rename: readers never
        // see half a file, and concurrent puts of one key never share a temp.
        let mut tag = [0u8; 8];
        OsRandom.fill(&mut tag)?;
        let tag: String = tag.iter().map(|b| format!("{b:02x}")).collect();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let tmp = p.with_file_name(format!(".{name}.{tag}.tmp"));
        if let Err(e) = tokio::fs::write(&tmp, bytes).await {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(io(e));
        }
        tokio::fs::rename(&tmp, &p).await.map_err(io)
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match tokio::fs::read(self.path(key)?).await {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(e)),
        }
    }

    async fn delete(&self, key: &str) -> Result<()> {
        match tokio::fs::remove_file(self.path(key)?).await {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
            _ => Ok(()),
        }
    }
}
