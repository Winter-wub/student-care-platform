//! Self-hosted binary for the fictional Deployment.

use std::sync::Arc;

use example_deployment::{Secrets, with_design_a};
use harpseal_channel_line::LineConfig;
use harpseal_channel_telegram::TelegramConfig;
use harpseal_runtime_native::{NativeSettings, serve, services};

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
    // Plain HTTP on localhost: browsers drop `Secure` cookies there (Safari).
    let secure = env("HARPSEAL_SECURE_COOKIES", "0") == "1";

    let (services, db) = services(&settings)?;
    let d = with_design_a::deployment(services, secrets(), secure).map_err(|e| format!("{e:?}"))?;
    db.migrate(d.migrations())?;
    let router = Arc::new(d).router();
    println!("listening on http://{addr}  (try /admin/login, /app?student=68010001, /notice)");
    serve(router, &addr).await?;
    Ok(())
}
