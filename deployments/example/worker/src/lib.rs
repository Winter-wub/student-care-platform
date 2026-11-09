//! The fictional Seal Bay University Deployment as a Cloudflare Worker.
//!
//! `#[event(fetch)]` serves the Deployment's axum Router and
//! `#[event(scheduled)]` (Cron Trigger `* * * * *`) runs its per-minute
//! tick; both come from `harpseal-runtime-cloudflare`, which builds the
//! Deployment once per isolate and applies its migrations to D1. Built by
//! `wrangler.toml`'s `[build]` command (worker-build, then `wasm-opt -Oz`).

#![warn(missing_docs)]
#![forbid(unsafe_code)]

use example_deployment::{FAKE_DEMO_VAR, Secrets, hosted_deployment};
use harpseal_runtime_cloudflare::worker::{
    self, Context, Env, HttpRequest, ScheduleContext, ScheduledEvent, event,
};
use harpseal_runtime_cloudflare::{CloudflareSettings, env_value};

/// Bindings: D1 `DB`, R2 `FILES` (optional), secret `RESEND_API_KEY`,
/// var `EMAIL_FROM`. No Fake Channel (#133) unless var
/// `HARPSEAL_FAKE_DEMO` is `"1"` (a hosted demo only, with a `FAKE_SECRET`
/// of at least 16 characters): see `example_deployment::hosted_deployment`. LINE's rich menu images are the PNGs bundled in the
/// Deployment crate (`example_deployment::menu_images`, about 116 KB of the
/// wasm): Workers have no files, so there is no `LINE_MENU_IMAGE_*`
/// override here yet (a customer bundles its own bytes or reads an asset
/// binding).
fn settings() -> CloudflareSettings {
    CloudflareSettings::new()
}

fn build(services: harpseal::Services, env: &Env) -> Result<harpseal::Deployment, String> {
    let secrets = Secrets::from_lookup(|k| env_value(env, k));
    let fake_demo = env_value(env, FAKE_DEMO_VAR).is_some_and(|v| v == "1");
    hosted_deployment(services, secrets, fake_demo).map_err(|e| e.to_string())
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    ctx: Context,
) -> worker::Result<harpseal_runtime_cloudflare::Response> {
    harpseal_runtime_cloudflare::fetch(req, &env, ctx, &settings(), build).await
}

#[event(scheduled)]
async fn scheduled(event: ScheduledEvent, env: Env, ctx: ScheduleContext) {
    harpseal_runtime_cloudflare::scheduled(event, &env, &ctx, &settings(), build).await;
}
