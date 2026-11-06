//! Self-hosted binary for the fictional Seal Bay University Deployment:
//! `cargo run -p harpseal-example-native`, then open
//! <http://127.0.0.1:8080/notice>.

#![forbid(unsafe_code)]

use std::sync::Arc;

use example_deployment::{Secrets, deployment};
use harpseal_channel_line::LineConfig;
use harpseal_channel_telegram::TelegramConfig;
use harpseal_runtime_native::{NativeSettings, serve, services};

fn env(k: &str, default: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| default.to_string())
}

fn secrets() -> Result<Secrets, String> {
    let fake_secret = std::env::var("FAKE_SECRET")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or("FAKE_SECRET must be set (the Fake Channel's webhook secret)")?;
    Ok(Secrets {
        line: LineConfig::new(
            env("LINE_CHANNEL_SECRET", ""),
            env("LINE_ACCESS_TOKEN", ""),
            env("LINE_LIFF_ID", ""),
        ),
        telegram: TelegramConfig::new(
            env("TELEGRAM_BOT_TOKEN", ""),
            env("TELEGRAM_WEBHOOK_SECRET", ""),
            env("TELEGRAM_WEB_APP_URL", ""),
        ),
        fake_secret,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = NativeSettings::new(
        env("HARPSEAL_DB", "example.sqlite3"),
        env("HARPSEAL_FILES", "files"),
        env("HARPSEAL_SMTP", "smtp://localhost:25"),
    );
    let addr = env("HARPSEAL_ADDR", "127.0.0.1:8080");
    let d = deployment(services(&settings)?, secrets()?)?;
    let report = d.migrate().await?;
    println!(
        "migrations: {} applied, {} already done",
        report.applied.len(),
        report.skipped
    );
    println!("listening on http://{addr}  (try /notice)");
    serve(Arc::new(d).router(), &addr).await?;
    Ok(())
}
