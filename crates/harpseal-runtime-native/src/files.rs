//! [`LocalFiles`]: a directory on the server's disk as the `FileStore`.

use std::path::PathBuf;

use async_trait::async_trait;
use harpseal::{Error, FileStore, Result};

/// A local directory as the FileStore. Keys are relative paths with `/`
/// separators; empty keys, absolute keys and `..` segments are refused.
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
        let bad = key.is_empty()
            || key.starts_with('/')
            || key.contains('\\')
            || key
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..");
        if bad {
            return Err(Error::Invalid(format!("bad file key {key:?}")));
        }
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
        // Write a sibling temp file, then rename: readers never see half a file.
        let tmp = p.with_extension(format!(
            "{}.tmp",
            p.extension().and_then(|e| e.to_str()).unwrap_or("")
        ));
        tokio::fs::write(&tmp, bytes).await.map_err(io)?;
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
