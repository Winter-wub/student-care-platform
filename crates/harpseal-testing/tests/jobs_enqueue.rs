//! Enqueueing and scheduled Jobs at the Deployment seam (#23).

mod jobs_support;

use std::sync::atomic::Ordering;

use harpseal::{
    BuildError, Error, Job, JobContext, JobSpec, Module, ModuleRegistrar, Statement, Timestamp,
};
use harpseal_testing::Harness;
use jobs_support::{MIN, setup, setup_with};
use serde_json::json;

#[tokio::test]
async fn enqueued_jobs_run_when_due_with_their_payload() {
    let s = setup().await;
    let now = s.now();
    let id =
        s.d.enqueue("demo.ok", json!({ "n": 1 }), Some(Timestamp(now + 5 * MIN)))
            .await
            .unwrap();
    let rows = s
        .db()
        .query(Statement::new("SELECT payload FROM job WHERE id = ?").bind(id.0.as_str()))
        .await
        .unwrap();
    assert_eq!(
        rows.get(0, "payload").unwrap().as_text(),
        Some(r#"{"n":1}"#)
    );
    assert_eq!(s.tick(now).await.claimed, 0);
    assert_eq!(s.tick(now + 5 * MIN).await.done, 1);
    assert_eq!(s.job(&id.0).await.state, "done");

    let soon = s.d.enqueue("demo.ok", json!({}), None).await.unwrap();
    assert_eq!(s.job(&soon.0).await.run_at, now, "run_at defaults to now");
}

#[tokio::test]
async fn an_unregistered_kind_is_refused() {
    let s = setup().await;
    let e = s.d.enqueue("demo.nope", json!({}), None).await.unwrap_err();
    assert!(
        matches!(&e, Error::Invalid(m) if m.contains("demo.nope")),
        "{e}"
    );
    assert!(s.d.jobs().insert("demo.nope", &json!({}), None).is_err());
    assert!(
        s.d.enqueue_unique("k", "demo.nope", json!({}), None)
            .await
            .is_err()
    );
    assert_eq!(s.count("SELECT count(*) FROM job").await, 0);
}

#[tokio::test]
async fn enqueue_unique_dedupes_on_its_key() {
    let s = setup().await;
    let first =
        s.d.enqueue_unique("report-2026-11", "demo.ok", json!({}), None)
            .await;
    let first = first.unwrap().expect("first enqueue inserts");
    let again =
        s.d.enqueue_unique("report-2026-11", "demo.ok", json!({}), None)
            .await;
    assert_eq!(again.unwrap(), None, "same key: nothing inserted");
    s.tick(s.now()).await;
    let after_done =
        s.d.enqueue_unique("report-2026-11", "demo.ok", json!({}), None)
            .await;
    assert_eq!(
        after_done.unwrap(),
        None,
        "the key holds after the Job is done"
    );
    assert_eq!(s.job(&first.0).await.state, "done");
    let other =
        s.d.enqueue_unique("report-2026-12", "demo.ok", json!({}), None)
            .await;
    assert!(other.unwrap().is_some());
    assert!(
        s.d.enqueue_unique("", "demo.ok", json!({}), None)
            .await
            .is_err()
    );
    let long = "k".repeat(201);
    assert!(
        s.d.enqueue_unique(&long, "demo.ok", json!({}), None)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_module_enqueues_inside_its_own_batch() {
    let s = setup().await;
    let (id, insert) =
        s.d.jobs()
            .insert("demo.ok", &json!({ "chunk": 1 }), None)
            .unwrap();
    // The batch fails as a whole: the Job is not left behind.
    let bad = Statement::new("INSERT INTO no_such_table VALUES (1)");
    assert!(s.db().batch(vec![insert.clone(), bad]).await.is_err());
    assert_eq!(s.count("SELECT count(*) FROM job").await, 0);
    s.db().batch(vec![insert]).await.unwrap();
    assert_eq!(s.job(&id.0).await.state, "queued");
}

#[tokio::test]
async fn a_scheduled_job_runs_once_per_due_minute_across_ticks() {
    let s = setup_with(true).await;
    let t0 = s.now(); // 00:00Z = 07:00 at +07:00: both schedules are due
    let a = s.tick(t0).await;
    assert_eq!((a.scheduled, a.done), (2, 2));
    let b = s.tick(t0 + 30_000).await;
    assert_eq!((b.scheduled, b.claimed), (0, 0), "second tick, same minute");
    let (c, d) = tokio::join!(
        s.d.tick(Timestamp(t0 + 40_000)),
        s.d.tick(Timestamp(t0 + 50_000))
    );
    assert_eq!(c.unwrap().scheduled + d.unwrap().scheduled, 0);
    assert_eq!(s.scheduled.load(Ordering::SeqCst), 1);

    // 00:01 to 00:04: not due. The 00:01 tick sees 00:00 again (look-back)
    // and inserts nothing.
    for m in 1..5 {
        assert_eq!(s.tick(t0 + m * MIN).await.scheduled, 0, "minute {m}");
    }
    assert_eq!(s.tick(t0 + 5 * MIN + 10_000).await.scheduled, 1);
    assert_eq!(s.scheduled.load(Ordering::SeqCst), 2);
    let runs = s.of_kind("clock.every5").await;
    assert_eq!(
        runs,
        vec![("done".into(), t0), ("done".into(), t0 + 5 * MIN)],
        "run_at is the minute the schedule named"
    );
    assert_eq!(
        s.of_kind("clock.morning").await.len(),
        1,
        "07:00 local only"
    );
}

#[tokio::test]
async fn a_tick_that_slips_past_a_due_minute_still_enqueues_it() {
    let s = setup_with(true).await;
    let t0 = s.now();
    s.tick(t0).await;
    // No tick at 00:05; the next one comes at 00:06:02.
    let r = s.tick(t0 + 6 * MIN + 2_000).await;
    assert_eq!((r.scheduled, r.done), (1, 1));
    let runs = s.of_kind("clock.every5").await;
    assert_eq!(runs.last().map(|r| r.1), Some(t0 + 5 * MIN));
    // Two minutes late is skipped, not caught up.
    let r = s.tick(t0 + 12 * MIN).await;
    assert_eq!(r.scheduled, 0, "00:10 was two minutes ago");
}

struct Cron(&'static str);

impl Module for Cron {
    fn name(&self) -> &'static str {
        "cron"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.job(
            JobSpec::scheduled("cron.x", self.0),
            |_cx: JobContext, _job: Job| async move { Ok(()) },
        );
    }
}

#[test]
fn an_unsupported_schedule_is_refused_at_build_time() {
    let h = Harness::new();
    for bad in [
        "@daily",
        "0 7 * * MON",
        "*/5 * * *",
        "0-30/5 * * * *",
        "61 * * * *",
    ] {
        let e = h.builder().module(Cron(bad)).build().unwrap_err();
        assert!(
            matches!(&e, BuildError::InvalidSchedule { kind, schedule, .. }
                if kind == "cron.x" && schedule == bad),
            "{bad}: {e:?}"
        );
    }
    h.builder()
        .module(Cron("0,30 8-17 * * 1-5"))
        .build()
        .unwrap();
}

struct Twice;

impl Module for Twice {
    fn name(&self) -> &'static str {
        "twice"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        let h = |_cx: JobContext, _job: Job| async move { Ok(()) };
        r.job(JobSpec::new("twice.x"), h)
            .job(JobSpec::new("twice.x"), h);
    }
}

#[test]
fn a_job_kind_registered_twice_is_refused() {
    let e = Harness::new().builder().module(Twice).build().unwrap_err();
    assert_eq!(e, BuildError::DuplicateJob("twice.x".into()));
}
