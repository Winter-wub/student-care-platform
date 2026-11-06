//! Cloudflare Workers runtime via workers-rs.
//!
//! [`services`] builds the Core's `Services` from the Worker `Env`:
//! D1 -> `Db`, `worker::Fetch` -> `HttpClient`, `Date.now` -> `Clock`,
//! `crypto.getRandomValues` (getrandom `wasm_js`) -> `Random`. R2 ->
//! `FileStore` and Resend email arrive with #18: until then the FileStore
//! returns `Error::Platform` and `EmailSender` only logs.
//!
//! JS-backed futures are `!Send`; the Core's traits require `Send`, so every
//! impl wraps them in `worker::send::SendFuture`. That is sound here because
//! a Worker isolate is single-threaded.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod d1;

use std::sync::Arc;

use async_trait::async_trait;
use harpseal::{Email, Error, Result, Services, Timestamp};
use worker::js_sys::{self, Uint8Array};
use worker::send::SendFuture;
use worker::{Env, Fetch, Headers, Method, Request, RequestInit};

pub use d1::D1Db;
/// workers-rs, re-exported so a Worker crate uses the same version.
pub use worker;

fn platform(e: impl core::fmt::Display) -> Error {
    Error::Platform(e.to_string())
}

/// `Date.now()`. Workers freeze it during CPU work and advance it on I/O.
pub struct WorkerClock;

impl harpseal::Clock for WorkerClock {
    fn now(&self) -> Timestamp {
        Timestamp(js_sys::Date::now() as i64)
    }
}

/// `crypto.getRandomValues` through getrandom's `wasm_js` backend.
pub struct WorkerRandom;

impl harpseal::Random for WorkerRandom {
    fn fill(&self, buf: &mut [u8]) -> Result<()> {
        getrandom::fill(buf).map_err(platform)
    }
}

/// Outgoing HTTP through `fetch`.
pub struct WorkerHttp;

#[async_trait]
impl harpseal::HttpClient for WorkerHttp {
    async fn send(&self, req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
        SendFuture::new(async move {
            let (parts, body) = req.into_parts();
            let headers = Headers::new();
            for (k, v) in &parts.headers {
                let v = v.to_str().map_err(platform)?;
                headers.set(k.as_str(), v).map_err(platform)?;
            }
            let mut init = RequestInit::new();
            init.with_method(Method::from(parts.method.to_string()))
                .with_headers(headers);
            if !body.is_empty() {
                init.with_body(Some(Uint8Array::from(body.as_slice()).into()));
            }
            let req = Request::new_with_init(&parts.uri.to_string(), &init).map_err(platform)?;
            let mut resp = Fetch::Request(req).send().await.map_err(platform)?;
            let mut out = http::Response::builder().status(resp.status_code());
            for (k, v) in resp.headers().entries() {
                out = out.header(k, v);
            }
            let bytes = resp.bytes().await.map_err(platform)?;
            out.body(bytes).map_err(platform)
        })
        .await
    }
}

/// Logs instead of sending. Resend over `fetch` arrives with #18.
pub struct LogEmail;

#[async_trait]
impl harpseal::EmailSender for LogEmail {
    async fn send(&self, email: Email) -> Result<()> {
        worker::console_log!("email to={} subject={:?}", email.to, email.subject);
        Ok(())
    }
}

/// R2-backed `FileStore`. The bucket binding arrives with #18.
pub struct R2Files;

fn no_r2() -> Error {
    Error::Platform("R2 FileStore arrives with #18".into())
}

#[async_trait]
impl harpseal::FileStore for R2Files {
    async fn put(&self, _key: &str, _bytes: Vec<u8>, _ct: &str) -> Result<()> {
        Err(no_r2())
    }
    async fn get(&self, _key: &str) -> Result<Option<Vec<u8>>> {
        Err(no_r2())
    }
    async fn delete(&self, _key: &str) -> Result<()> {
        Err(no_r2())
    }
}

/// Builds `Services` from the Worker `Env`. `d1_binding` is the
/// `[[d1_databases]] binding` name in `wrangler.toml`.
pub fn services(env: &Env, d1_binding: &str) -> Result<Services> {
    let db = env.d1(d1_binding).map_err(platform)?;
    Ok(Services {
        db: Arc::new(D1Db::new(db)),
        files: Arc::new(R2Files),
        email: Arc::new(LogEmail),
        http: Arc::new(WorkerHttp),
        clock: Arc::new(WorkerClock),
        random: Arc::new(WorkerRandom),
    })
}
