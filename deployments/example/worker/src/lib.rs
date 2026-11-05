//! The fictional Deployment as a Cloudflare Worker (issue #5 prototype).
//! `#[event(fetch)]` hands every request to the Deployment's axum router
//! (workers-rs `http` feature), plus the `/bench/*` measurement routes.
//! Built with `worker-build --release`; see `wrangler.toml`.

mod bench;

use std::cell::RefCell;
use std::sync::Arc;

use example_deployment::{Secrets, with_design_a};
use harpseal_channel_line::LineConfig;
use harpseal_channel_telegram::TelegramConfig;
use tower_service::Service;
use worker::{Context, Env, HttpRequest, event};

pub use example_deployment::DEPLOYMENT_TOML;
pub use harpseal_runtime_cloudflare::worker;

thread_local! {
    // One isolate = one thread: build the router on the first request and
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
        line: LineConfig {
            channel_secret: var(env, "LINE_CHANNEL_SECRET"),
            access_token: var(env, "LINE_ACCESS_TOKEN"),
            liff_id: var(env, "LINE_LIFF_ID"),
        },
        telegram: TelegramConfig {
            bot_token: var(env, "TELEGRAM_BOT_TOKEN"),
            webhook_secret: var(env, "TELEGRAM_WEBHOOK_SECRET"),
            web_app_url: var(env, "TELEGRAM_WEB_APP_URL"),
        },
        fake_secret: var(env, "FAKE_SECRET"),
    }
}

fn build(env: &Env) -> worker::Result<axum::Router> {
    let services = harpseal_runtime_cloudflare::services(env, "DB")
        .map_err(|e| worker::Error::RustError(e.to_string()))?;
    let d = with_design_a::deployment(services.clone(), secrets(env), true)
        .map_err(|e| worker::Error::RustError(format!("{e:?}")))?;
    Ok(Arc::new(d).router().merge(bench::router(services)))
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
