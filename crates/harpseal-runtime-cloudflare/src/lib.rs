//! Cloudflare Workers runtime via workers-rs.
//!
//! [`services`] builds the Core's `Services` from the Worker `Env` and
//! [`CloudflareSettings`]:
//!
//! | Port | Implementation |
//! |---|---|
//! | `Db` | [`D1Db`] (D1 binding; `batch` = `D1Database::batch`, atomic) |
//! | `FileStore` | [`R2Files`] (R2 binding; errors on use when the binding is absent) |
//! | `EmailSender` | [`harpseal::ResendEmail`] over `fetch` when the `RESEND_API_KEY` secret is set, otherwise [`NoEmail`] (every send is an error) |
//! | `HttpClient` | [`WorkerHttp`] (`fetch`) |
//! | `Clock` | [`WorkerClock`] (`Date.now()`, **frozen while the request computes**) |
//! | `Random` | [`WorkerRandom`] (`crypto.getRandomValues` via getrandom `wasm_js`) |
//!
//! [`fetch`] and [`scheduled`] are the bodies of a Worker's `#[event(fetch)]`
//! and `#[event(scheduled)]` handlers: they build the Deployment once per
//! isolate (running its migrations on D1), then serve the axum Router or
//! call `Deployment::tick` from the Cron Trigger.
//!
//! JS-backed futures and objects are `!Send`; the Core's traits require
//! `Send`, so every impl wraps them in `worker::send::SendFuture` /
//! `SendWrapper`. That is sound here because a Worker isolate runs one
//! thread. A panic is logged by [`install_panic_hook`] (the release
//! profile aborts on panic, which would otherwise end the request silently).

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod d1;
mod entry;
mod http;
mod r2;

use std::sync::Arc;

use async_trait::async_trait;
use harpseal::{Email, Error, Result, Services, Timestamp};
use worker::Env;
use worker::js_sys;

pub use d1::D1Db;
pub use entry::{BuildFn, Response, fetch, scheduled};
pub use http::WorkerHttp;
pub use r2::R2Files;
/// workers-rs, re-exported so a Worker crate uses the same version.
pub use worker;

pub(crate) fn platform(e: impl core::fmt::Display) -> Error {
    Error::Platform(e.to_string())
}

/// `Date.now()`. Workers freeze it while the request computes and advance
/// it only on I/O, so it reads the same before and after CPU work: never
/// measure durations with it (`wrangler tail` reports CPU time).
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkerClock;

impl harpseal::Clock for WorkerClock {
    fn now(&self) -> Timestamp {
        Timestamp(js_sys::Date::now() as i64)
    }
}

/// `crypto.getRandomValues` through getrandom's `wasm_js` backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkerRandom;

impl harpseal::Random for WorkerRandom {
    fn fill(&self, buf: &mut [u8]) -> Result<()> {
        getrandom::fill(buf).map_err(platform)
    }
}

/// The EmailSender when no email provider is configured: every send fails
/// with [`Error::Platform`] naming the missing secret, so an OTP is never
/// reported as sent when it was not.
#[derive(Debug, Clone)]
pub struct NoEmail {
    missing: String,
}

#[async_trait]
impl harpseal::EmailSender for NoEmail {
    async fn send(&self, email: Email) -> Result<()> {
        worker::console_error!(
            "email to {} not sent: {} is not set",
            email.to,
            self.missing
        );
        Err(Error::Platform(format!(
            "email is not configured: set the {} secret",
            self.missing
        )))
    }
}

/// Binding and secret names this runtime reads from the Worker `Env`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CloudflareSettings {
    /// `[[d1_databases]] binding`. Default `DB`.
    pub d1_binding: String,
    /// `[[r2_buckets]] binding`. Default `FILES`; absent is allowed.
    pub r2_binding: String,
    /// Secret holding the Resend API key. Default `RESEND_API_KEY`.
    pub resend_key_secret: String,
    /// Var (or secret) holding the sender address. Default `EMAIL_FROM`.
    pub email_from_var: String,
}

impl Default for CloudflareSettings {
    fn default() -> Self {
        Self {
            d1_binding: "DB".into(),
            r2_binding: "FILES".into(),
            resend_key_secret: "RESEND_API_KEY".into(),
            email_from_var: "EMAIL_FROM".into(),
        }
    }
}

impl CloudflareSettings {
    /// The defaults: `DB`, `FILES`, `RESEND_API_KEY`, `EMAIL_FROM`.
    pub fn new() -> Self {
        Self::default()
    }
    /// Another D1 binding name.
    pub fn d1(mut self, binding: impl Into<String>) -> Self {
        self.d1_binding = binding.into();
        self
    }
    /// Another R2 binding name.
    pub fn r2(mut self, binding: impl Into<String>) -> Self {
        self.r2_binding = binding.into();
        self
    }
}

/// A secret, else a var, else `None` (empty values count as unset). Use it
/// as the lookup for a Deployment's secrets.
pub fn env_value(env: &Env, name: &str) -> Option<String> {
    env.secret(name)
        .map(|s| s.to_string())
        .or_else(|_| env.var(name).map(|v| v.to_string()))
        .ok()
        .filter(|v| !v.is_empty())
}

/// Builds `Services` from the Worker `Env`. Fails only when the D1 binding
/// is missing.
pub fn services(env: &Env, s: &CloudflareSettings) -> Result<Services> {
    let db = env
        .d1(&s.d1_binding)
        .map_err(|e| Error::Platform(format!("no D1 binding `{}`: {e}", s.d1_binding)))?;
    let http: Arc<dyn harpseal::HttpClient> = Arc::new(WorkerHttp);
    let email: Arc<dyn harpseal::EmailSender> = match env_value(env, &s.resend_key_secret) {
        Some(key) => {
            let from =
                env_value(env, &s.email_from_var).unwrap_or_else(|| "noreply@localhost".into());
            Arc::new(harpseal::ResendEmail::new(http.clone(), key, from))
        }
        None => Arc::new(NoEmail {
            missing: s.resend_key_secret.clone(),
        }),
    };
    Ok(Services {
        db: Arc::new(D1Db::new(db)),
        files: Arc::new(R2Files::from_env(env, &s.r2_binding)),
        email,
        http,
        clock: Arc::new(WorkerClock),
        random: Arc::new(WorkerRandom),
    })
}

/// Logs panics with `console.error` (visible in `wrangler tail` and
/// `wrangler dev`). Idempotent; [`fetch`] and [`scheduled`] call it.
pub fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            worker::console_error!("panic: {info}");
        }));
    });
}
