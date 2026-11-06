//! `GET /admin/jobs.json` (#23): guarded by `kernel.audit.view`, JSON.

mod jobs_support;

use harpseal::{Deployment, DeploymentConfig, EmailOtp};
use harpseal_testing::{Harness, TestApp};
use jobs_support::{Demo, MIN};
use serde_json::Value;

const PASSWORD: &str = "tide pools at noon";

const CONFIG: &str = r#"
name = "Seal Bay University (test)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Administrator = ["kernel.audit.view", "kernel.student.view"]
Viewer = ["kernel.student.view"]
"#;

async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let d: Deployment = h
        .builder()
        .config(config.clone())
        .verification(EmailOtp::new(config.otp))
        .module(Demo {
            runs: Default::default(),
        })
        .build()
        .unwrap();
    let app = h.boot(d).await;
    (h, app)
}

#[tokio::test]
async fn a_viewer_gets_403_and_no_session_is_sent_to_login() {
    let (h, app) = boot().await;
    let anon = app.get("/admin/jobs.json").await;
    assert_eq!(anon.status, 303);
    assert_eq!(anon.header("location"), Some("/admin/login"));
    h.create_staff("viewer@sealbay.example", PASSWORD, &["Viewer"])
        .await;
    let b = app.browser();
    b.login("viewer@sealbay.example", PASSWORD).await;
    let r = b.get("/admin/jobs.json").await;
    assert_eq!(r.status, 403);
    assert!(!r.body.contains("\"counts\""));
}

#[tokio::test]
async fn an_administrator_gets_counts_and_recent_failed_and_running_jobs() {
    let (h, app) = boot().await;
    let d = app.deployment();
    let now = h.clock().now_ms();
    d.enqueue("demo.ok", serde_json::json!({ "secret": "payload" }), None)
        .await
        .unwrap();
    d.enqueue("demo.flaky", serde_json::json!({}), None)
        .await
        .unwrap();
    let later = harpseal::Timestamp(now + 60 * MIN);
    d.enqueue("demo.ok", serde_json::json!({}), Some(later))
        .await
        .unwrap();
    app.query("UPDATE job SET progress = 'from an earlier attempt' WHERE kind = 'demo.flaky'")
        .await;
    d.tick(harpseal::Timestamp(now)).await.unwrap();
    // The flaky Job ran once and is given up; 60 older failures; one Job
    // running now (claimed by a live tick, newest run_at).
    app.query("UPDATE job SET state = 'failed', finished_at = 1 WHERE kind = 'demo.flaky'")
        .await;
    app.query(
        "INSERT INTO job (id, kind, run_at, state, attempts, last_error, created_at, finished_at) \
         WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 60) \
         SELECT printf('old-%02d', i), 'demo.flaky', i, 'failed', 4, 'old', 0, 1 FROM n",
    )
    .await;
    let lock = now + 15 * MIN;
    h.db()
        .execute(
            harpseal::Statement::new(
                "INSERT INTO job (id, kind, run_at, state, attempts, lock_token, locked_until, \
                 progress, created_at) VALUES ('live', 'demo.ok', ?, 'running', 1, 't', ?, \
                 'chunk 1 of 4', ?)",
            )
            .bind(now + 2 * MIN)
            .bind(lock)
            .bind(now),
        )
        .await
        .unwrap();

    h.create_staff("admin@sealbay.example", PASSWORD, &["Administrator"])
        .await;
    let b = app.browser();
    b.login("admin@sealbay.example", PASSWORD).await;
    let r = b.get("/admin/jobs.json").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.header("content-type"), Some("application/json"));
    assert_eq!(r.header("cache-control"), Some("no-store"));
    let v: Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(v["now"], now);
    assert_eq!(
        v["counts"],
        serde_json::json!({ "queued": 1, "running": 1, "done": 1, "failed": 61 })
    );
    let recent = v["recent"].as_array().unwrap();
    assert_eq!(recent.len(), 50, "capped at 50");
    assert_eq!(recent[0]["id"], "live");
    assert_eq!(recent[0]["state"], "running");
    assert_eq!(recent[0]["locked_until"], lock);
    assert_eq!(recent[0]["progress"], "chunk 1 of 4");
    assert_eq!(recent[1]["kind"], "demo.flaky");
    assert_eq!(recent[1]["state"], "failed");
    assert_eq!(recent[1]["attempts"], 1);
    assert_eq!(recent[1]["last_error"], "platform: attempt 1 failed");
    assert_eq!(recent[1]["locked_until"], Value::Null);
    assert_eq!(
        recent[1]["progress"],
        Value::Null,
        "the claim cleared the old note"
    );
    assert_eq!(recent[49]["id"], "old-13", "newest run_at first");
    assert!(!r.body.contains("payload"), "payloads are never listed");
}
