//! A Job builds the Mini App URL of a page (#163), for an email: the URL a
//! Mini App button to that path opens, per Channel, from the real LINE
//! and Telegram adapters. A Channel without a Mini App, one not
//! registered and a dev-only one outside dev give none.

use harpseal::{
    Deployment, EmailOtp, Job, JobContext, JobSpec, Module, ModuleRegistrar, Statement, Timestamp,
};
use harpseal_channel_line::{LineChannel, LineConfig};
use harpseal_channel_telegram::{TelegramChannel, TelegramConfig};
use harpseal_testing::{Harness, TestApp};

const LIFF: &str = "2000000000-SealBay0";
const WEB_APP: &str = "https://sealbay.example";
const PAGE: &str = "/app/dues";

/// A Job that saves, as its progress, the URL its payload asks for:
/// `{"kind": "line"}` one Channel's (`none` without), `{}` every Channel's
/// kind.
struct Probe;

impl Module for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        r.job(JobSpec::new("probe.url"), url);
    }
}

async fn url(cx: JobContext, job: Job) -> harpseal::Result<()> {
    let p: serde_json::Value = serde_json::from_str(&job.payload).unwrap();
    let note = match p["kind"].as_str() {
        Some(kind) => cx.mini_app_url(kind, PAGE).unwrap_or_else(|| "none".into()),
        None => {
            let all = cx.mini_app_urls(PAGE);
            let kinds: Vec<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
            kinds.join(",")
        }
    };
    cx.progress(&note).await
}

async fn boot(liff: &str, web_app: &str, dev: bool) -> (Harness, TestApp) {
    let h = Harness::new();
    let s = h.services();
    let line = LineChannel::new(
        LineConfig::new("line-secret", "line-token", liff),
        s.http.clone(),
    );
    let tg = TelegramChannel::new(
        TelegramConfig::new("8000000001:AAFsealBay", "tg-secret", web_app),
        s.http.clone(),
    )
    .with_clock(s.clock.clone());
    let d = Deployment::builder()
        .dev(dev)
        .config(Harness::config())
        .runtime(s)
        .channel(line)
        .channel(tg)
        .channel(h.fake_channel())
        .verification(EmailOtp::new(Harness::config().otp))
        .module(Probe)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    (h, app)
}

/// Runs `probe.url` with `payload`; its progress note.
async fn run(h: &Harness, app: &TestApp, payload: serde_json::Value) -> String {
    let d = app.deployment();
    d.enqueue("probe.url", payload, None).await.unwrap();
    let r = d.tick(Timestamp(h.clock().now_ms())).await.unwrap();
    assert_eq!(r.failed, 0, "{:?}", r.errors);
    h.clock().advance_ms(1);
    let rows = h
        .db()
        .query(Statement::new(
            "SELECT progress FROM job ORDER BY created_at DESC, id DESC",
        ))
        .await
        .unwrap();
    rows.get(0, "progress")
        .and_then(|v| v.as_text())
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn a_job_gets_the_mini_app_url_of_a_page_on_line_and_telegram() {
    let (h, app) = boot(LIFF, WEB_APP, false).await;
    let line = run(&h, &app, serde_json::json!({ "kind": "line" })).await;
    assert_eq!(line, format!("https://liff.line.me/{LIFF}{PAGE}"));
    let tg = run(&h, &app, serde_json::json!({ "kind": "telegram" })).await;
    assert_eq!(tg, format!("{WEB_APP}{PAGE}"));
    // Not registered, or the Fake Channel outside dev: none.
    for kind in ["viber", "fake"] {
        let none = run(&h, &app, serde_json::json!({ "kind": kind })).await;
        assert_eq!(none, "none", "{kind}");
    }
    // Every Channel with a Mini App, in registration order.
    assert_eq!(run(&h, &app, serde_json::json!({})).await, "line,telegram");
    // The same from the Deployment and as a Mini App button would open it.
    let d = app.deployment();
    let button = d.channel("line").unwrap().mini_app().unwrap();
    assert_eq!(d.mini_app_url("line", PAGE), Some(button.launch_url(PAGE)));
}

#[tokio::test]
async fn a_channel_without_a_mini_app_gives_no_url() {
    // No LIFF id, no Web App URL; in dev the Fake Channel's Mini App counts.
    let (h, app) = boot("", "", true).await;
    for kind in ["line", "telegram"] {
        let none = run(&h, &app, serde_json::json!({ "kind": kind })).await;
        assert_eq!(none, "none", "{kind}");
    }
    assert_eq!(run(&h, &app, serde_json::json!({})).await, "fake");
}
