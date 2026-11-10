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
//! [`serve`] (or [`serve_until`]) runs the Router with `axum::serve`, handling webhooks on a
//! [`NativeBackground`] after the 200; [`spawn_ticker`] calls
//! [`Deployment::tick`] once a minute.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod background;
mod email;
mod files;
mod http;
mod s3;
mod settings;
mod sqlite;

use std::sync::Arc;
use std::time::Duration;

use harpseal::{Clock, Deployment, Error, Random, Result, Services, Timestamp};

pub use background::{BACKGROUND_LIMIT, NativeBackground};
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

/// How long [`serve_until`] waits, after it stopped taking requests, for
/// webhook handling still running in the background.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(8);

/// `axum::serve` on a TCP listener at `addr`, until the process is killed.
/// Each request carries the peer's IP as [`harpseal::PeerIp`] (audit rows
/// fall back to it when there is no `X-Forwarded-For`) and a
/// [`harpseal::Background`]: a webhook is answered 200 once its events are
/// claimed, and handled on a [`NativeBackground`] task
/// ([`BACKGROUND_LIMIT`] events at once, one at a time per chat account), so a
/// client closing the connection does not stop it. A response carrying a
/// [`harpseal::ErrorReport`] logs its line and `x-request-id` to stderr
/// (the area and the error kind, never the message).
///
/// Handling cut off by a kill leaves its claim unfinished; the next
/// process's tick logs it as `inbound.lost` (ADR-0009). Use
/// [`serve_until`] to let it finish on shutdown.
pub async fn serve(router: axum::Router, addr: &str) -> std::io::Result<()> {
    serve_until(router, addr, std::future::pending()).await
}

/// [`serve`] until `shutdown` completes (for example a SIGTERM future):
/// then it stops taking connections, lets requests in flight finish,
/// waits up to [`SHUTDOWN_GRACE`] for background handling, and returns.
pub async fn serve_until(
    router: axum::Router,
    addr: &str,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bg = NativeBackground::new(BACKGROUND_LIMIT);
    let app = router
        .layer(axum::Extension(bg.background()))
        .layer(axum::middleware::map_response(log_report))
        .layer(axum::middleware::from_fn(peer_ip));
    let app = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    if !bg.wait_idle(SHUTDOWN_GRACE).await {
        eprintln!(
            "shutdown: {} webhook task(s) still running; the tick will log them as inbound.lost",
            bg.pending()
        );
    }
    Ok(())
}

async fn log_report(r: axum::response::Response) -> axum::response::Response {
    if let Some(line) = harpseal::ErrorReport::log_line(&r) {
        eprintln!("{line}");
    }
    r
}

async fn peer_ip(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    req.extensions_mut()
        .insert(harpseal::PeerIp(addr.ip().to_string()));
    next.run(req).await
}

/// Calls [`Deployment::tick`] every `period` (one minute in production)
/// with the Deployment's Clock, starting one period from now: the same
/// entry the Workers `scheduled` event calls. Each tick runs in its own
/// task. A handler panic is caught by the tick and recorded as a failed
/// run of that Job (retried like any failure). A tick that did anything
/// (any count non-zero, or errors) logs its `TickReport` line to stderr,
/// the same line the Workers entry logs; idle ticks stay quiet. An error, a failed outcome
/// write or a panic elsewhere in the tick is printed to stderr and the
/// next tick runs anyway. A tick that
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
                Ok(Ok(r)) => {
                    if r != harpseal::TickReport::default() {
                        eprintln!("tick: {r}");
                    }
                    r.errors.iter().for_each(|e| eprintln!("tick: {e}"));
                }
                Ok(Err(e)) => eprintln!("tick failed: {e}"),
                Err(e) => eprintln!("tick aborted: {e}"),
            }
        }
    })
}
