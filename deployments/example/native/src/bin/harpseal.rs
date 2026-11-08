//! The `harpseal` CLI for the fictional Seal Bay University Deployment,
//! against the native runtime (`NativeSettings::from_env`, so the same
//! `HARPSEAL_DB` the server uses):
//!
//! ```text
//! cargo run -p harpseal-example-native --bin harpseal -- migrate
//! cargo run -p harpseal-example-native --bin harpseal -- \
//!     staff create --admin --email admin@sealbay.example
//! cargo run -p harpseal-example-native --bin harpseal -- \
//!     seed-example [--as admin@sealbay.example] [--force]
//! ```
//!
//! `seed-example` is this Deployment's own command (demo data, see
//! `example_deployment::seed`); everything else is `harpseal_cli::run`.
//! It is for demo databases only: while it waits for the roster import it
//! runs every due Job here, with no Fake Channel and the real EmailSender,
//! so on a live database pending announcement deliveries and Quota emails
//! would run from the CLI. `--force` prints a warning saying so.
//!
//! It reads the same environment as the server (`HARPSEAL_DB`, …). LINE and
//! Telegram tokens are not needed, but `LINE_LIFF_ID` and
//! `TELEGRAM_WEB_APP_URL` are, as for the server: the Deployment refuses to
//! build without them. The CLI builds it without the Fake Channel.
//! `seed-example` runs due Jobs while it waits for the roster import.

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
    let d = builder(services(&settings)?, secrets)?.build()?;
    if args.first().map(String::as_str) == Some("seed-example") {
        let (email, force) = seed_args(&args[1..])?;
        if force {
            eprintln!(
                "warning: seed-example --force is for demo databases only: it runs every due Job \
                 (announcement deliveries, Quota emails) from this CLI"
            );
        }
        let report = example_deployment::seed::seed(&d, email.as_deref(), force).await?;
        println!("{report}");
        return Ok(());
    }
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    harpseal_cli::run(&d, &args, &mut input, &mut std::io::stdout()).await?;
    Ok(())
}

/// `seed-example [--as EMAIL] [--force]`.
fn seed_args(rest: &[String]) -> Result<(Option<String>, bool), String> {
    let usage = "usage: harpseal seed-example [--as EMAIL] [--force]";
    let (mut email, mut force) = (None, false);
    let mut it = rest.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--as" => email = Some(it.next().ok_or(usage)?.clone()),
            "--force" => force = true,
            _ => return Err(usage.into()),
        }
    }
    Ok((email, force))
}
