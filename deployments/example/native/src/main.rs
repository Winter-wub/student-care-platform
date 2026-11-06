//! Self-hosted binary for the fictional Seal Bay University Deployment:
//! `FAKE_SECRET=change-me cargo run -p harpseal-example-native`, then open
//! <http://127.0.0.1:8080/notice>. Every setting comes from the environment
//! (`NativeSettings::from_env`, `Secrets::from_lookup`); see the README.

#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use example_deployment::{Secrets, deployment};
use harpseal_runtime_native::{NativeSettings, serve, services, spawn_ticker};

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
    let secrets = Secrets::from_lookup(|k| std::env::var(k).ok());
    let d = Arc::new(deployment(services(&settings)?, secrets)?);
    let report = d.migrate().await?;
    println!(
        "migrations: {} applied, {} already done",
        report.applied.len(),
        report.skipped
    );
    let _ticker = spawn_ticker(d.clone(), Duration::from_secs(60));
    println!("listening on http://{}  (try /notice)", settings.addr);
    serve(d.router(), &settings.addr).await?;
    Ok(())
}
