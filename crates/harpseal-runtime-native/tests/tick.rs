//! `Deployment::tick` at the Deployment seam (harness fakes), and the
//! native ticker calling it.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use harpseal::{Error, Job, JobContext, JobSpec, Module, ModuleRegistrar, Statement, Timestamp};
use harpseal_testing::Harness;

static RUNS: AtomicUsize = AtomicUsize::new(0);

/// `demo.ok` succeeds; `demo.flaky` always fails.
struct Demo;

impl Module for Demo {
    fn name(&self) -> &'static str {
        "demo"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        let ok = JobSpec {
            kind: "demo.ok",
            schedule: None,
        };
        let flaky = JobSpec {
            kind: "demo.flaky",
            schedule: None,
        };
        r.job(ok, |_cx: JobContext, _job: Job| async move {
            RUNS.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .job(flaky, |_cx: JobContext, job: Job| async move {
            Err(Error::Platform(format!("attempt {} failed", job.attempt)))
        });
    }
}

async fn queue(app: &harpseal_testing::TestApp, id: &str, kind: &str, run_at: i64) {
    let db = app.deployment().services().db.clone();
    db.execute(
        Statement::new(
            "INSERT INTO job (id, kind, run_at, max_attempts, created_at) VALUES (?, ?, ?, 2, ?)",
        )
        .bind(id)
        .bind(kind)
        .bind(run_at)
        .bind(run_at),
    )
    .await
    .unwrap();
}

async fn state(app: &harpseal_testing::TestApp, id: &str) -> (String, i64, i64) {
    let rows = app
        .deployment()
        .services()
        .db
        .query(Statement::new("SELECT state, attempts, run_at FROM job WHERE id = ?").bind(id))
        .await
        .unwrap();
    let text = rows.get(0, "state").and_then(|v| v.as_text()).unwrap();
    let int = |c| rows.get(0, c).and_then(|v| v.as_i64()).unwrap();
    (text.to_string(), int("attempts"), int("run_at"))
}

#[tokio::test]
async fn tick_runs_due_jobs_and_retries_failures() {
    let h = Harness::new();
    let app = h.boot(h.builder().module(Demo).build().unwrap()).await;
    let now = h.clock().now_ms();
    queue(&app, "job-due", "demo.ok", now - 1).await;
    queue(&app, "job-later", "demo.ok", now + 60_000).await;
    queue(&app, "job-flaky", "demo.flaky", now).await;

    let before = RUNS.load(Ordering::SeqCst);
    let r = app.deployment().tick(Timestamp(now)).await.unwrap();
    assert_eq!((r.claimed, r.done, r.retried, r.failed), (2, 1, 1, 0));
    assert_eq!(RUNS.load(Ordering::SeqCst) - before, 1);
    assert_eq!(state(&app, "job-due").await.0, "done");
    assert_eq!(state(&app, "job-later").await.0, "queued");
    assert_eq!(
        state(&app, "job-flaky").await,
        ("queued".into(), 1, now + 60_000)
    );

    // Nothing due a moment later; at +1 min the later job and the retry run.
    let r = app.deployment().tick(Timestamp(now + 1)).await.unwrap();
    assert_eq!(r.claimed, 0);
    let r = app
        .deployment()
        .tick(Timestamp(now + 60_000))
        .await
        .unwrap();
    assert_eq!((r.claimed, r.done, r.failed), (2, 1, 1));
    assert_eq!(state(&app, "job-flaky").await.0, "failed");
}

#[tokio::test]
async fn native_ticker_calls_tick() {
    let h = Harness::new();
    let app = h.boot(h.builder().module(Demo).build().unwrap()).await;
    queue(&app, "job-tick", "demo.ok", h.clock().now_ms()).await;
    let d = Arc::new(h.builder().module(Demo).build().unwrap());
    let ticker = harpseal_runtime_native::spawn_ticker(d, Duration::from_millis(20));
    for _ in 0..100 {
        if state(&app, "job-tick").await.0 == "done" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    ticker.abort();
    assert_eq!(state(&app, "job-tick").await.0, "done");
}
