//! Self-hosted binary for the fictional Seal Bay University Deployment:
//! `FAKE_SECRET=change-me cargo run -p harpseal-example-native`, then open
//! <http://127.0.0.1:8080/notice>. Every setting comes from the environment
//! (`NativeSettings::from_env`, `Secrets::from_lookup`, plus the optional
//! `LINE_MENU_IMAGE_LOCKED` / `LINE_MENU_IMAGE_UNLOCKED` PNG paths); see
//! `docs/guide/self-hosting.md`.
//!
//! `HARPSEAL_DEV_ROUTES=1` turns on dev mode for demos and `just e2e`:
//! email is captured instead of sent and `/dev/*` routes, guarded by
//! `FAKE_SECRET`, show the Fake Channel's outbox and the last email and
//! run a tick on demand (see [`dev`]). It refuses to start on a
//! non-loopback address or with a `FAKE_SECRET` shorter than 16 characters
//! or equal to `change-me`. Never set it in production.
//!
//! In dev mode only, `HARPSEAL_DEV_FIXED_OTP=<digits>` makes every Student
//! OTP that code (still hashed, emailed, expiring and attempt-counted), so
//! a demo needs no inbox; set without dev mode, the binary refuses to start.

#![forbid(unsafe_code)]

mod dev;

use std::sync::Arc;
use std::time::Duration;

use example_deployment::{Secrets, deployment_with_fake_otp};
use harpseal_runtime_native::{
    DEV_FIXED_OTP_WARNING, NativeSettings, dev_fixed_otp, serve, services, spawn_ticker,
};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let settings = NativeSettings::from_env()?;
    let env = |k: &str| std::env::var(k).ok();
    // LINE_MENU_IMAGE_LOCKED / _UNLOCKED: PNG files instead of the bundled rich menu images.
    let secrets = Secrets::from_lookup(env).with_menu_image_files(env, |p| std::fs::read(p))?;
    let secret = secrets.fake_secret.clone();
    let dev_mode = std::env::var("HARPSEAL_DEV_ROUTES").is_ok_and(|v| v == "1");
    // HARPSEAL_DEV_FIXED_OTP: refused (not ignored) outside dev mode.
    let fixed_otp = dev_fixed_otp(env, dev_mode)?;
    let mut services = services(&settings)?;
    let captured = dev::CapturingEmail::default();
    if dev_mode {
        dev::check_safe(&settings.addr, &secret)?;
        services.email = Arc::new(captured.clone());
    }
    let (d, fake) = deployment_with_fake_otp(services, secrets, dev_mode, fixed_otp.as_deref())?;
    if fixed_otp.is_some() {
        eprintln!("{DEV_FIXED_OTP_WARNING}");
    }
    let d = Arc::new(d);
    let report = d.migrate().await?;
    println!(
        "migrations: {} applied, {} already done",
        report.applied.len(),
        report.skipped
    );
    for n in &report.notices {
        eprintln!("notice: {n}");
    }
    let _ticker = spawn_ticker(d.clone(), Duration::from_secs(60));
    let mut router = d.clone().router();
    if dev_mode {
        eprintln!("dev mode: emails captured, /dev routes on (never in production)");
        router = router.merge(dev::router(d.clone(), fake, captured, secret));
    }
    println!("listening on http://{}  (try /notice)", settings.addr);
    serve(router, &settings.addr).await?;
    Ok(())
}
