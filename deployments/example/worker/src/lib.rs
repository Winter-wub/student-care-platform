//! The fictional Seal Bay University Deployment as a Cloudflare Worker.
//! `#[event(fetch)]` hands every request to the Deployment's axum Router
//! (workers-rs `http` feature). Built with `worker-build --release`; see
//! `wrangler.toml`. Migrations on D1 and the Cron trigger arrive with #18.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::sync::Arc;

use example_deployment::{Secrets, deployment};
use harpseal_channel_line::LineConfig;
use harpseal_channel_telegram::TelegramConfig;
use harpseal_runtime_cloudflare::worker::{self, Context, Env, HttpRequest, event};
use tower_service::Service;

thread_local! {
    // One isolate = one thread: build the Router on the first request and
    // reuse it while the isolate lives (warm requests skip the build).
    static ROUTER: RefCell<Option<axum::Router>> = const { RefCell::new(None) };
}

fn var(env: &Env, k: &str) -> String {
    env.secret(k)
        .map(|s| s.to_string())
        .or_else(|_| env.var(k).map(|v| v.to_string()))
        .unwrap_or_default()
}

fn secrets(env: &Env) -> Secrets {
    Secrets {
        line: LineConfig::new(
            var(env, "LINE_CHANNEL_SECRET"),
            var(env, "LINE_ACCESS_TOKEN"),
            var(env, "LINE_LIFF_ID"),
        ),
        telegram: TelegramConfig::new(
            var(env, "TELEGRAM_BOT_TOKEN"),
            var(env, "TELEGRAM_WEBHOOK_SECRET"),
            var(env, "TELEGRAM_WEB_APP_URL"),
        ),
        fake_secret: var(env, "FAKE_SECRET"),
    }
}

fn build(env: &Env) -> worker::Result<axum::Router> {
    let services = harpseal_runtime_cloudflare::services(env, "DB")
        .map_err(|e| worker::Error::RustError(e.to_string()))?;
    let d =
        deployment(services, secrets(env)).map_err(|e| worker::Error::RustError(e.to_string()))?;
    Ok(Arc::new(d).router())
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: Context,
) -> worker::Result<axum::http::Response<axum::body::Body>> {
    let cached = ROUTER.with(|r| r.borrow().clone());
    let mut router = match cached {
        Some(r) => r,
        None => {
            let r = build(&env)?;
            ROUTER.with(|c| *c.borrow_mut() = Some(r.clone()));
            r
        }
    };
    let resp = router.call(req).await;
    Ok(resp.unwrap_or_else(|e| match e {}))
}
