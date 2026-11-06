//! The native ticker calling `Deployment::tick`. The Job lifecycle itself
//! (claims, retries, stale locks, scheduling, pruning) is tested at the
//! Deployment seam in `harpseal-testing/tests/jobs*.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use harpseal::{Deployment, Job, JobContext, JobSpec, Module, ModuleRegistrar, Statement};
use harpseal_testing::Harness;

/// `demo.ok` counts its runs; `demo.panic` panics.
struct Demo {
    runs: Arc<AtomicUsize>,
}

impl Module for Demo {
    fn name(&self) -> &'static str {
        "demo"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        let runs = self.runs.clone();
        r.job(
            JobSpec::new("demo.ok"),
            move |_cx: JobContext, _job: Job| {
                let runs = runs.clone();
                async move {
                    runs.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            },
        )
        .job(
            JobSpec::new("demo.panic"),
            |_cx: JobContext, _job: Job| async move { panic!("demo.panic handler panicked") },
        );
    }
}

struct Setup {
    h: Harness,
    d: Arc<Deployment>,
    runs: Arc<AtomicUsize>,
}

async fn setup() -> Setup {
    let h = Harness::new();
    let runs = Arc::new(AtomicUsize::new(0));
    let d = h
        .builder()
        .module(Demo { runs: runs.clone() })
        .build()
        .unwrap();
    d.migrate().await.unwrap();
    Setup {
        d: Arc::new(d),
        h,
        runs,
    }
}

impl Setup {
    async fn queue(&self, id: &str, kind: &str) {
        let now = self.h.clock().now_ms();
        self.h
            .db()
            .execute(
                Statement::new(
                    "INSERT INTO job (id, kind, run_at, created_at) VALUES (?, ?, ?, ?)",
                )
                .bind(id)
                .bind(kind)
                .bind(now)
                .bind(now),
            )
            .await
            .unwrap();
    }
    /// `(state, last_error)`.
    async fn job(&self, id: &str) -> (String, Option<String>) {
        let rows = self
            .h
            .db()
            .query(Statement::new("SELECT state, last_error FROM job WHERE id = ?").bind(id))
            .await
            .unwrap();
        let text = |c| rows.get(0, c).and_then(|v| v.as_text()).map(String::from);
        (text("state").unwrap(), text("last_error"))
    }
    async fn wait_for(&self, id: &str, state: &str) -> bool {
        for _ in 0..200 {
            if self.job(id).await.0 == state {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }
}

#[tokio::test]
async fn native_ticker_calls_tick() {
    let s = setup().await;
    s.queue("job-tick", "demo.ok").await;
    let ticker = harpseal_runtime_native::spawn_ticker(s.d.clone(), Duration::from_millis(20));
    assert!(s.wait_for("job-tick", "done").await);
    assert_eq!(s.runs.load(Ordering::SeqCst), 1);
    ticker.abort();
}

#[tokio::test]
async fn native_ticker_records_a_handler_panic_as_a_failed_run() {
    let s = setup().await;
    s.queue("job-panic", "demo.panic").await;
    let ticker = harpseal_runtime_native::spawn_ticker(s.d.clone(), Duration::from_millis(20));
    // The ManualClock does not move, so the retry (1 min later) never comes.
    for _ in 0..200 {
        if s.job("job-panic").await.1.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (state, err) = s.job("job-panic").await;
    assert_eq!(state, "queued", "retried, not stuck in running");
    assert_eq!(
        err.as_deref(),
        Some("platform: job handler panicked: demo.panic handler panicked")
    );
    assert!(!ticker.is_finished(), "the ticker is still alive");
    s.queue("job-after", "demo.ok").await;
    assert!(
        s.wait_for("job-after", "done").await,
        "later ticks still run"
    );
    ticker.abort();
}
