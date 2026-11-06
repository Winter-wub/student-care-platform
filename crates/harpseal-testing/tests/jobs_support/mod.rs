//! Shared setup for the Job runner tests (#23): a `demo` Module with Jobs
//! that succeed, fail, panic, report progress and run on a schedule, and
//! helpers to queue and read back `job` rows.

#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use harpseal::{
    Db, Deployment, Error, Job, JobContext, JobSpec, Module, ModuleRegistrar, Statement,
    TickReport, Timestamp,
};
use harpseal_testing::Harness;

pub const MIN: i64 = 60_000;
pub const DAY: i64 = 24 * 60 * MIN;

/// `demo.ok` counts its runs; `demo.flaky` always fails; `demo.panic`
/// panics; `demo.progress` writes a progress note.
pub struct Demo {
    pub runs: Arc<AtomicUsize>,
}

/// `clock.every5` (counted) runs on `*/5 * * * *` and `clock.morning` at
/// 07:00 in the Deployment's offset.
pub struct Clockwork {
    pub runs: Arc<AtomicUsize>,
}

impl Module for Clockwork {
    fn name(&self) -> &'static str {
        "clock"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        let runs = self.runs.clone();
        r.job(
            JobSpec::scheduled("clock.every5", "*/5 * * * *"),
            move |_cx: JobContext, _job: Job| {
                let runs = runs.clone();
                async move {
                    runs.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            },
        )
        .job(
            JobSpec::scheduled("clock.morning", "0 7 * * *"),
            |_cx: JobContext, _job: Job| async move { Ok(()) },
        );
    }
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
            JobSpec::new("demo.flaky"),
            |_cx: JobContext, job: Job| async move {
                Err(Error::Platform(format!("attempt {} failed", job.attempt)))
            },
        )
        .job(
            JobSpec::new("demo.panic"),
            |_cx: JobContext, _job: Job| async move { panic!("demo.panic handler panicked") },
        )
        .job(
            JobSpec::new("demo.progress"),
            |cx: JobContext, _job: Job| async move {
                cx.progress("chunk 2 of 3").await?;
                Err(Error::Platform("stopped after chunk 2".into()))
            },
        );
    }
}

pub struct Setup {
    pub h: Harness,
    pub d: Arc<Deployment>,
    pub runs: Arc<AtomicUsize>,
    pub scheduled: Arc<AtomicUsize>,
}

/// A migrated Deployment with [`Demo`].
pub async fn setup() -> Setup {
    setup_with(false).await
}

/// [`setup`], plus [`Clockwork`] when `scheduled`. The clock starts at
/// 2026-11-05T00:00Z = 07:00 in the test config's `+07:00`.
pub async fn setup_with(scheduled_jobs: bool) -> Setup {
    let h = Harness::new();
    let runs = Arc::new(AtomicUsize::new(0));
    let scheduled = Arc::new(AtomicUsize::new(0));
    let mut b = h.builder().module(Demo { runs: runs.clone() });
    if scheduled_jobs {
        b = b.module(Clockwork {
            runs: scheduled.clone(),
        });
    }
    let d = b.build().unwrap();
    d.migrate().await.unwrap();
    Setup {
        d: Arc::new(d),
        h,
        runs,
        scheduled,
    }
}

/// One `job` row as a later request reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: String,
    pub state: String,
    pub attempts: i64,
    pub run_at: i64,
    pub last_error: Option<String>,
    pub progress: Option<String>,
}

impl Setup {
    pub fn db(&self) -> Arc<dyn Db> {
        self.h.db()
    }
    pub fn now(&self) -> i64 {
        self.h.clock().now_ms()
    }
    /// A queued Job with the schema's default `max_attempts` (3 retries),
    /// inserted directly so ids sort in the order the test needs.
    pub async fn queue(&self, id: &str, kind: &str, run_at: i64) {
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
    pub async fn orphan(&self, id: &str, attempts: i64) {
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
    pub async fn job(&self, id: &str) -> Row {
        let rows = self
            .db()
            .query(
                Statement::new(
                    "SELECT kind, state, attempts, run_at, last_error, progress FROM job WHERE id = ?",
                )
                .bind(id),
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "job {id} exists");
        let text = |c| rows.get(0, c).and_then(|v| v.as_text()).map(String::from);
        let int = |c| rows.get(0, c).and_then(|v| v.as_i64()).unwrap();
        Row {
            kind: text("kind").unwrap(),
            state: text("state").unwrap(),
            attempts: int("attempts"),
            run_at: int("run_at"),
            last_error: text("last_error"),
            progress: text("progress"),
        }
    }
    /// Rows of `kind`, oldest first: `(state, run_at)`.
    pub async fn of_kind(&self, kind: &str) -> Vec<(String, i64)> {
        let rows = self
            .db()
            .query(
                Statement::new("SELECT state, run_at FROM job WHERE kind = ? ORDER BY run_at")
                    .bind(kind),
            )
            .await
            .unwrap();
        (0..rows.len())
            .map(|i| {
                (
                    rows.get(i, "state").unwrap().as_text().unwrap().to_string(),
                    rows.get(i, "run_at").unwrap().as_i64().unwrap(),
                )
            })
            .collect()
    }
    pub async fn count(&self, sql: &'static str) -> i64 {
        let rows = self.db().query(Statement::new(sql)).await.unwrap();
        rows.rows[0][0].as_i64().unwrap()
    }
    pub async fn tick(&self, at: i64) -> TickReport {
        self.d.tick(Timestamp(at)).await.unwrap()
    }
}
