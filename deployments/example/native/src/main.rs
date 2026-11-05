//! Self-hosted binary for the fictional Deployment. `HARPSEAL_DESIGN=b`
//! picks the Design B wiring; anything else uses Design A.

use std::sync::Arc;

use example_deployment::{Secrets, with_design_a, with_design_b};
use harpseal_channel_line::LineConfig;
use harpseal_channel_telegram::TelegramConfig;
use harpseal_runtime_native::{NativeHttp, NativeRuntime, NativeSettings, serve, services};

fn env(k: &str, default: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| default.to_string())
}

fn secrets() -> Secrets {
    Secrets {
        line: LineConfig {
            channel_secret: env("LINE_CHANNEL_SECRET", ""),
            access_token: env("LINE_ACCESS_TOKEN", ""),
            liff_id: env("LINE_LIFF_ID", ""),
        },
        telegram: TelegramConfig {
            bot_token: env("TELEGRAM_BOT_TOKEN", ""),
            webhook_secret: env("TELEGRAM_WEBHOOK_SECRET", ""),
            web_app_url: env("TELEGRAM_WEB_APP_URL", ""),
        },
        fake_secret: env("FAKE_SECRET", "demo"),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = NativeSettings {
        sqlite_path: env("HARPSEAL_DB", "example.sqlite3"),
        files_root: env("HARPSEAL_FILES", "files").into(),
        smtp_url: env("HARPSEAL_SMTP", "smtp://localhost:25"),
    };
    let addr = env("HARPSEAL_ADDR", "127.0.0.1:8080");

    let router = if env("HARPSEAL_DESIGN", "a") == "b" {
        let d = with_design_b::deployment(NativeRuntime::open(&settings)?, NativeHttp, secrets())
            .map_err(|e| format!("{e:?}"))?;
        d.runtime().db.migrate(d.migrations())?;
        Arc::new(d).router()
    } else {
        let (services, db) = services(&settings)?;
        let d = with_design_a::deployment(services, secrets()).map_err(|e| format!("{e:?}"))?;
        db.migrate(d.migrations())?;
        Arc::new(d).router()
    };
    serve(router, &addr).await?;
    Ok(())
}
