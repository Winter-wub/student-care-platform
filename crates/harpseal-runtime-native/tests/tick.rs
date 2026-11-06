//! `Deployment::tick` at the Deployment seam (harness fakes), and the
//! native ticker calling it.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use harpseal::{
    Db, Deployment, Error, Job, JobContext, JobSpec, Module, ModuleRegistrar, Statement, Timestamp,
};
use harpseal_testing::Harness;

const MIN: i64 = 60_000;

/// `demo.ok` counts its runs; `demo.flaky` always fails; `demo.panic`
/// panics. Each test owns its counter.
struct Demo {
    runs: Arc<AtomicUsize>,
}

impl Module for Demo {
    fn name(&self) -> &'static str {
        "demo"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        let spec = |kind| JobSpec {
            kind,
            schedule: None,
        };
        let runs = self.runs.clone();
        r.job(spec("demo.ok"), move |_cx: JobContext, _job: Job| {
            let runs = runs.clone();
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .job(spec("demo.flaky"), |_cx: JobContext, job: Job| async move {
            Err(Error::Platform(format!("attempt {} failed", job.attempt)))
        })
        .job(
            spec("demo.panic"),
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
    fn db(&self) -> Arc<dyn Db> {
        self.h.db()
    }
    fn now(&self) -> i64 {
        self.h.clock().now_ms()
    }
    /// A queued Job with the schema's default `max_attempts` (3 retries).
    async fn queue(&self, id: &str, kind: &str, run_at: i64) {
        self.db()
            .execute(
                Statement::new(
                    "INSERT INTO job (id, kind, run_at, created_at) VALUES (?, ?, ?, ?)",
                )
                .bind(id)
                .bind(kind)
                .bind(run_at)
                .bind(run_at),
            )
            .await
            .unwrap();
    }
    /// A Job some runtime claimed and then died on: `running`, lock expired.
    async fn orphan(&self, id: &str, attempts: i64) {
        let now = self.now();
        self.db()
            .execute(
                Statement::new(
                    "INSERT INTO job (id, kind, run_at, state, attempts, lock_token, \
                     locked_until, created_at) VALUES (?, 'demo.ok', ?, 'running', ?, 'dead', ?, ?)",
                )
                .bind(id)
                .bind(now - 20 * MIN)
                .bind(attempts)
                .bind(now - 1)
                .bind(now - 20 * MIN),
            )
            .await
            .unwrap();
    }
    /// `(state, attempts, run_at, last_error)`.
    async fn job(&self, id: &str) -> (String, i64, i64, Option<String>) {
        let rows = self
            .db()
            .query(
                Statement::new("SELECT state, attempts, run_at, last_error FROM job WHERE id = ?")
                    .bind(id),
            )
            .await
            .unwrap();
        let text = |c| rows.get(0, c).and_then(|v| v.as_text()).map(String::from);
        let int = |c| rows.get(0, c).and_then(|v| v.as_i64()).unwrap();
        (
            text("state").unwrap(),
            int("attempts"),
            int("run_at"),
            text("last_error"),
        )
    }
    async fn tick(&self, at: i64) -> harpseal::TickReport {
        self.d.tick(Timestamp(at)).await.unwrap()
    }
}

#[tokio::test]
async fn tick_runs_due_jobs_and_leaves_later_ones() {
    let s = setup().await;
    let t0 = s.now();
    s.queue("01-due", "demo.ok", t0 - 1).await;
    s.queue("02-later", "demo.ok", t0 + MIN).await;
    let r = s.tick(t0).await;
    assert_eq!((r.claimed, r.done, r.retried, r.failed), (1, 1, 0, 0));
    assert!(r.errors.is_empty());
    assert_eq!(s.runs.load(Ordering::SeqCst), 1);
    assert_eq!(s.job("01-due").await.0, "done");
    assert_eq!(s.job("02-later").await.0, "queued");
    assert_eq!(s.tick(t0 + 1).await.claimed, 0);
    assert_eq!(s.tick(t0 + MIN).await.done, 1);
}

#[tokio::test]
async fn failures_retry_after_1_5_and_30_minutes_then_fail() {
    let s = setup().await;
    let t0 = s.now();
    s.queue("flaky", "demo.flaky", t0).await;

    assert_eq!(s.tick(t0).await.retried, 1);
    let (state, attempts, run_at, err) = s.job("flaky").await;
    assert_eq!((state.as_str(), attempts, run_at), ("queued", 1, t0 + MIN));
    assert_eq!(err.as_deref(), Some("platform: attempt 1 failed"));

    let t1 = t0 + MIN;
    assert_eq!(s.tick(t1).await.retried, 1);
    assert_eq!(s.job("flaky").await.2, t1 + 5 * MIN, "second step is 5 min");
    assert_eq!(s.tick(t1 + 4 * MIN).await.claimed, 0, "not due yet");

    let t2 = t1 + 5 * MIN;
    assert_eq!(s.tick(t2).await.retried, 1);
    assert_eq!(
        s.job("flaky").await.2,
        t2 + 30 * MIN,
        "third step is 30 min"
    );

    let r = s.tick(t2 + 30 * MIN).await;
    assert_eq!((r.claimed, r.failed), (1, 1));
    let (state, attempts, _, err) = s.job("flaky").await;
    assert_eq!(
        (state.as_str(), attempts),
        ("failed", 4),
        "3 retries = 4 runs"
    );
    assert_eq!(err.as_deref(), Some("platform: attempt 4 failed"));
    assert_eq!(s.tick(t2 + 300 * MIN).await.claimed, 0);
}

#[tokio::test]
async fn job_kind_without_handler_fails_after_retries_and_never_spins() {
    let s = setup().await;
    let mut at = s.now();
    s.queue("ghost", "demo.unregistered", at).await;
    for step in [MIN, 5 * MIN, 30 * MIN] {
        assert_eq!(s.tick(at).await.retried, 1);
        at += step;
    }
    assert_eq!(s.tick(at).await.failed, 1);
    let (state, attempts, _, err) = s.job("ghost").await;
    assert_eq!((state.as_str(), attempts), ("failed", 4));
    assert!(err.unwrap().contains("no job kind demo.unregistered"));
    for later in 1..5 {
        assert_eq!(s.tick(at + later * 60 * MIN).await.claimed, 0);
    }
}

#[tokio::test]
async fn expired_lock_is_reclaimed_while_retries_remain_then_failed() {
    let s = setup().await;
    let now = s.now();
    s.orphan("orphan-retry", 2).await;
    s.orphan("orphan-spent", 4).await; // 4 runs started, max 3 retries

    let r = s.tick(now).await;
    assert_eq!((r.claimed, r.done, r.failed), (1, 1, 1));
    assert_eq!(s.runs.load(Ordering::SeqCst), 1, "the spent Job is not run");
    let (state, attempts, _, _) = s.job("orphan-retry").await;
    assert_eq!((state.as_str(), attempts), ("done", 3));
    let (state, attempts, _, err) = s.job("orphan-spent").await;
    assert_eq!((state.as_str(), attempts), ("failed", 4));
    assert_eq!(err.as_deref(), Some("lock expired"));
    assert_eq!(s.tick(now + 60 * MIN).await.claimed, 0);
}

#[tokio::test]
async fn a_live_lock_is_not_reclaimed() {
    let s = setup().await;
    let now = s.now();
    s.orphan("busy", 1).await;
    s.db()
        .execute(Statement::new("UPDATE job SET locked_until = ?").bind(now + MIN))
        .await
        .unwrap();
    assert_eq!(s.tick(now).await.claimed, 0);
    assert_eq!(s.job("busy").await.0, "running");
}

#[tokio::test]
async fn two_ticks_at_once_claim_disjoint_jobs() {
    let s = setup().await;
    let now = s.now();
    for i in 0..30 {
        s.queue(&format!("job-{i:02}"), "demo.ok", now).await;
    }
    let (a, b) = tokio::join!(s.d.tick(Timestamp(now)), s.d.tick(Timestamp(now)));
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.claimed + b.claimed, 30);
    assert_eq!(a.done + b.done, 30);
    assert_eq!(
        s.runs.load(Ordering::SeqCst),
        30,
        "every Job ran exactly once"
    );
    let rows = s
        .db()
        .query(Statement::new(
            "SELECT count(*) AS n FROM job WHERE state = 'done' AND attempts = 1",
        ))
        .await
        .unwrap();
    assert_eq!(rows.get(0, "n").and_then(|v| v.as_i64()), Some(30));
}

async fn wait_for(s: &Setup, id: &str, state: &str) -> bool {
    for _ in 0..200 {
        if s.job(id).await.0 == state {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

#[tokio::test]
async fn native_ticker_calls_tick() {
    let s = setup().await;
    s.queue("job-tick", "demo.ok", s.now()).await;
    let ticker = harpseal_runtime_native::spawn_ticker(s.d.clone(), Duration::from_millis(20));
    assert!(wait_for(&s, "job-tick", "done").await);
    ticker.abort();
}

#[tokio::test]
async fn native_ticker_survives_a_handler_panic() {
    let s = setup().await;
    s.queue("job-panic", "demo.panic", s.now()).await;
    let ticker = harpseal_runtime_native::spawn_ticker(s.d.clone(), Duration::from_millis(20));
    assert!(wait_for(&s, "job-panic", "running").await, "claimed");
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(!ticker.is_finished(), "the ticker is still alive");
    s.queue("job-after", "demo.ok", s.now()).await;
    assert!(
        wait_for(&s, "job-after", "done").await,
        "later ticks still run"
    );
    ticker.abort();
}
