//! Native runtime: a single tokio binary on the customer's own server.
//! [`services`] builds the Core's `Services`: a SQLite file ([`SqliteDb`]),
//! a local directory as FileStore, the OS clock and RNG. SMTP / Resend
//! email and outgoing HTTP arrive with #18 and until then return
//! [`Error::Platform`].

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod sqlite;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use harpseal::{
    Clock, Email, EmailSender, Error, FileStore, HttpClient, Random, Result, Services, Timestamp,
};

pub use sqlite::SqliteDb;

/// A local directory as the FileStore.
pub struct LocalFiles {
    root: PathBuf,
}

impl LocalFiles {
    /// Files live under `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    fn path(&self, key: &str) -> Result<PathBuf> {
        if key.is_empty() || key.starts_with('/') || key.split('/').any(|p| p == "..") {
            return Err(Error::Invalid(format!("bad file key {key:?}")));
        }
        Ok(self.root.join(key))
    }
}

fn io(e: std::io::Error) -> Error {
    Error::Platform(e.to_string())
}

#[async_trait]
impl FileStore for LocalFiles {
    async fn put(&self, key: &str, bytes: Vec<u8>, _content_type: &str) -> Result<()> {
        let p = self.path(key)?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        std::fs::write(p, bytes).map_err(io)
    }
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.path(key)?) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(e)),
        }
    }
    async fn delete(&self, key: &str) -> Result<()> {
        match std::fs::remove_file(self.path(key)?) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
            _ => Ok(()),
        }
    }
}

/// Email over SMTP. Sending arrives with #18.
pub struct Smtp {
    #[allow(dead_code)] // read by the SMTP transport (#18)
    url: String,
}

#[async_trait]
impl EmailSender for Smtp {
    async fn send(&self, _email: Email) -> Result<()> {
        Err(Error::Platform("SMTP sending arrives with #18".into()))
    }
}

/// Outgoing HTTP. The client arrives with #18.
pub struct NativeHttp;

#[async_trait]
impl HttpClient for NativeHttp {
    async fn send(&self, _req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
        Err(Error::Platform(
            "native HTTP client arrives with #18".into(),
        ))
    }
}

/// The OS clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Timestamp(i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}

/// The OS RNG.
pub struct OsRandom;

impl Random for OsRandom {
    fn fill(&self, buf: &mut [u8]) -> Result<()> {
        getrandom::fill(buf).map_err(|e| Error::Platform(e.to_string()))
    }
}

/// Settings read from the environment / CLI flags.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NativeSettings {
    /// SQLite file path.
    pub sqlite_path: String,
    /// FileStore root directory.
    pub files_root: PathBuf,
    /// SMTP URL.
    pub smtp_url: String,
}

impl NativeSettings {
    /// Settings with the given paths.
    pub fn new(
        sqlite_path: impl Into<String>,
        files_root: impl Into<PathBuf>,
        smtp_url: impl Into<String>,
    ) -> Self {
        Self {
            sqlite_path: sqlite_path.into(),
            files_root: files_root.into(),
            smtp_url: smtp_url.into(),
        }
    }
}

/// Builds `Services` for `Deployment::builder().runtime(..)`.
pub fn services(s: &NativeSettings) -> Result<Services> {
    Ok(Services {
        db: Arc::new(SqliteDb::open(&s.sqlite_path)?),
        files: Arc::new(LocalFiles::new(s.files_root.clone())),
        email: Arc::new(Smtp {
            url: s.smtp_url.clone(),
        }),
        http: Arc::new(NativeHttp),
        clock: Arc::new(SystemClock),
        random: Arc::new(OsRandom),
    })
}

/// `axum::serve` on a TCP listener.
pub async fn serve(router: axum::Router, addr: &str) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await
}
