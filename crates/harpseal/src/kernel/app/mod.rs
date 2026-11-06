//! The Mini App (#28, spec "Mini App"): one page inside LINE (LIFF) or
//! Telegram (Web App) with the Student's Link status and announcements.
//!
//! Routes (public, no Staff session):
//! - `GET /` → `/app` (with the query: LIFF's `liff.state`);
//! - `GET /app`, `GET /app/announcements`: the Student's page;
//! - `GET /app/announcements/{id}`: one announcement (Card deep links);
//! - `POST /app/announcements/{id}/ack`: the one-way "รับทราบ";
//! - `POST /app/session`: platform token → App Session ([`exchange`]).
//!
//! Without a valid App Session every page answers the entry page, which
//! reads the platform token in the webview and posts it to
//! `/app/session` with its own path as `next`; the exchange redirects
//! back there. Sessions: [`session`]. The Kernel's scheduled Job
//! `kernel.app.prune_sessions` deletes rows expired over a day ago.

mod exchange;
mod pages;
mod sdk;
pub(crate) mod session;
mod texts;
mod view;

use axum::routing::{get, post};

pub(crate) use exchange::safe_next;

use crate::manifest::{Job, JobSpec};
use crate::message::Locale;
use crate::module::JobContext;
use crate::{AppState, ModuleRegistrar, Result, Services};

/// The Student's page.
pub(crate) const APP_PATH: &str = "/app";
/// The exchange.
pub(crate) const SESSION_PATH: &str = "/app/session";
/// Announcements (the Module's Mini App path).
pub(crate) const NEWS_PATH: &str = crate::modules::announcements::MINI_APP_PATH;
/// Appended to an announcement's path for its acknowledge endpoint.
pub(crate) const ACK_SUFFIX: &str = "/ack";
/// The pruning Job.
pub(crate) const PRUNE_JOB: &str = "kernel.app.prune_sessions";
/// Hourly, at minute 7.
const PRUNE_SCHEDULE: &str = "7 * * * *";

/// Registers the routes, texts and the pruning Job.
pub(crate) fn register(r: &mut ModuleRegistrar) {
    r.messages(texts::DEFAULTS)
        .public("/", get(pages::root))
        .public(APP_PATH, get(pages::home))
        .public(SESSION_PATH, post(exchange::exchange))
        .public(NEWS_PATH, get(pages::home))
        .public("/app/announcements/{id}", get(pages::detail))
        .public("/app/announcements/{id}/ack", post(pages::ack))
        .job(JobSpec::scheduled(PRUNE_JOB, PRUNE_SCHEDULE), prune_job);
}

/// Texts in `locale` with the `setting` overrides.
pub(crate) async fn texts(s: &AppState, locale: &Locale) -> Result<crate::kernel::text::Texts> {
    crate::kernel::text::Texts::load(&s.services, s.catalog.clone(), locale.clone()).await
}

/// Deletes App Sessions expired more than a day before `now` (ms); returns
/// how many. Run hourly by the Job `kernel.app.prune_sessions`.
pub(crate) async fn prune_sessions(services: &Services, now: i64) -> Result<u64> {
    session::prune(services, now).await
}

async fn prune_job(cx: JobContext, _job: Job) -> Result<()> {
    prune_sessions(&cx.services, cx.services.now_ms())
        .await
        .map(|_| ())
}
