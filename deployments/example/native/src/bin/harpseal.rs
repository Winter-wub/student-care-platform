//! The `harpseal` CLI for the fictional Seal Bay University Deployment,
//! against the native runtime (`NativeSettings::from_env`, so the same
//! `HARPSEAL_DB` the server uses):
//!
//! ```text
//! cargo run -p harpseal-example-native --bin harpseal -- migrate
//! cargo run -p harpseal-example-native --bin harpseal -- \
//!     staff create --admin --email admin@sealbay.example
//! ```
//!
//! Channel secrets are not needed: the CLI builds the Deployment without
//! the Fake Channel and never calls a platform.

#![forbid(unsafe_code)]

use example_deployment::{Secrets, builder};
use harpseal_runtime_native::{NativeSettings, services};

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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let settings = NativeSettings::from_env()?;
    let secrets = Secrets::from_lookup(|k| std::env::var(k).ok());
    let d = builder(services(&settings)?, secrets).build()?;
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    harpseal_cli::run(&d, &args, &mut input, &mut std::io::stdout()).await?;
    Ok(())
}
