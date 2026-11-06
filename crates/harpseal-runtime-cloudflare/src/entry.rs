//! The Worker entry points: [`fetch`] (HTTP) and [`scheduled`] (Cron).
//! Both build the Deployment once per isolate and reuse it while the
//! isolate lives, so warm requests skip the build and the migration check.

use std::cell::RefCell;
use std::sync::Arc;

use harpseal::{Deployment, Services, Timestamp};
use tower_service::Service;
use worker::{Env, HttpRequest, ScheduleContext, ScheduledEvent};

use crate::{CloudflareSettings, install_panic_hook, services};

/// What [`fetch`] answers: the Router's response.
pub type Response = http::Response<axum::body::Body>;

/// Builds the Deployment from the runtime's Services; the Worker reads its
/// own secrets from `Env` (see [`crate::env_value`]).
pub type BuildFn = fn(Services, &Env) -> Result<Deployment, String>;

#[derive(Clone)]
struct App {
    deployment: Arc<Deployment>,
    router: axum::Router,
}

thread_local! {
    // One isolate = one thread.
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn cached() -> Option<App> {
    APP.with(|a| a.borrow().clone())
}

fn rust_err(e: impl core::fmt::Display) -> worker::Error {
    worker::Error::RustError(e.to_string())
}

/// The cached App, or a new one: Services, build, migrate on D1.
async fn app(env: &Env, settings: &CloudflareSettings, build: BuildFn) -> worker::Result<App> {
    if let Some(app) = cached() {
        return Ok(app);
    }
    install_panic_hook();
    let d = build(services(env, settings).map_err(rust_err)?, env).map_err(rust_err)?;
    let d = Arc::new(d);
    if let Err(e) = d.migrate().await {
        // Another request in this isolate may have migrated meanwhile.
        return cached().ok_or_else(|| rust_err(format!("migrations failed: {e}")));
    }
    let app = App {
        router: d.clone().router(),
        deployment: d,
    };
    APP.with(|a| *a.borrow_mut() = Some(app.clone()));
    Ok(app)
}

/// The body of `#[event(fetch)]`: hands the request to the Deployment's
/// axum Router.
pub async fn fetch(
    req: HttpRequest,
    env: &Env,
    settings: &CloudflareSettings,
    build: BuildFn,
) -> worker::Result<Response> {
    let mut router = app(env, settings, build).await?.router;
    let resp = router.call(req).await;
    Ok(resp.unwrap_or_else(|e| match e {}))
}

/// The body of `#[event(scheduled)]`: runs `Deployment::tick` at the
/// trigger's scheduled time. Errors are logged (`wrangler tail`); the next
/// trigger runs anyway.
pub async fn scheduled(
    event: ScheduledEvent,
    env: &Env,
    _ctx: &ScheduleContext,
    settings: &CloudflareSettings,
    build: BuildFn,
) {
    let app = match app(env, settings, build).await {
        Ok(app) => app,
        Err(e) => {
            worker::console_error!("scheduled: could not start: {e}");
            return;
        }
    };
    let now = Timestamp(event.schedule() as i64);
    match app.deployment.tick(now).await {
        Ok(r) => worker::console_log!(
            "tick {}: claimed {}, done {}, retried {}, failed {}",
            event.cron(),
            r.claimed,
            r.done,
            r.retried,
            r.failed
        ),
        Err(e) => worker::console_error!("tick failed: {e}"),
    }
}
