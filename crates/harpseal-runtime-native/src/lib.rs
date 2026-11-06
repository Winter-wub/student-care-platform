//! Native runtime: a single tokio binary on the customer's own server.
//!
//! [`services`] builds the Core's `Services` from [`NativeSettings`]
//! (usually [`NativeSettings::from_env`]):
//!
//! | Port | Implementation |
//! |---|---|
//! | `Db` | [`SqliteDb`] (rusqlite, bundled SQLite; `batch` = one transaction) |
//! | `FileStore` | [`LocalFiles`] (a directory) or [`S3Files`] (S3-compatible, SigV4) |
//! | `EmailSender` | [`Smtp`] (lettre) or [`harpseal::ResendEmail`] (Resend HTTP API) |
//! | `HttpClient` | [`ReqwestHttp`] (reqwest, rustls) |
//! | `Clock` | [`SystemClock`] |
//! | `Random` | [`OsRandom`] (getrandom) |
//!
//! [`serve`] runs the Router with `axum::serve`; [`spawn_ticker`] calls
//! [`Deployment::tick`] once a minute.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod email;
mod files;
mod http;
mod s3;
mod settings;
mod sqlite;

use std::sync::Arc;
use std::time::Duration;

use harpseal::{Clock, Deployment, Error, Random, Result, Services, Timestamp};

pub use email::Smtp;
pub use files::LocalFiles;
pub use http::ReqwestHttp;
pub use s3::{S3Files, S3Settings};
pub use settings::{EmailSettings, FilesSettings, NativeSettings};
pub use sqlite::SqliteDb;

/// The OS clock.
#[derive(Debug, Clone, Copy, Default)]
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
#[derive(Debug, Clone, Copy, Default)]
pub struct OsRandom;

impl Random for OsRandom {
    fn fill(&self, buf: &mut [u8]) -> Result<()> {
        getrandom::fill(buf).map_err(|e| Error::Platform(e.to_string()))
    }
}

/// Builds `Services` for `Deployment::builder().runtime(..)`. Opens (or
/// creates) the SQLite file; nothing else connects until first use.
pub fn services(s: &NativeSettings) -> Result<Services> {
    let http: Arc<dyn harpseal::HttpClient> = Arc::new(ReqwestHttp::new()?);
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let files: Arc<dyn harpseal::FileStore> = match &s.files {
        FilesSettings::Local(root) => Arc::new(LocalFiles::new(root.clone())),
        FilesSettings::S3(s3) => Arc::new(S3Files::new(s3.clone(), http.clone(), clock.clone())?),
    };
    let email: Arc<dyn harpseal::EmailSender> = match &s.email {
        EmailSettings::Smtp { url, from } => Arc::new(Smtp::new(url, from)?),
        EmailSettings::Resend { api_key, from } => Arc::new(harpseal::ResendEmail::new(
            http.clone(),
            api_key.clone(),
            from.clone(),
        )),
    };
    Ok(Services {
        db: Arc::new(SqliteDb::open(&s.sqlite_path)?),
        files,
        email,
        http,
        clock,
        random: Arc::new(OsRandom),
    })
}

/// `axum::serve` on a TCP listener at `addr`.
pub async fn serve(router: axum::Router, addr: &str) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await
}

/// Calls [`Deployment::tick`] every `period` (one minute in production)
/// with the Deployment's Clock, starting one period from now. Each tick
/// runs in its own task: an error, a failed outcome write or a handler
/// panic is printed to stderr and the next tick runs anyway. A tick that
/// overruns delays the next one instead of piling up. Abort the handle to
/// stop.
pub fn spawn_ticker(d: Arc<Deployment>, period: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let start = tokio::time::Instant::now() + period;
        let mut every = tokio::time::interval_at(start, period);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            every.tick().await;
            let d = d.clone();
            let run = tokio::spawn(async move {
                let now = d.services().clock.now();
                d.tick(now).await
            });
            match run.await {
                Ok(Ok(r)) => r.errors.iter().for_each(|e| eprintln!("tick: {e}")),
                Ok(Err(e)) => eprintln!("tick failed: {e}"),
                Err(e) => eprintln!("tick aborted: {e}"),
            }
        }
    })
}
