//! The Job lifecycle at the Deployment seam (#23): claim, retry/backoff,
//! stale locks, exclusive claims under concurrent ticks, panics, pruning.

mod jobs_support;

use std::sync::atomic::Ordering;

use harpseal::Timestamp;
use jobs_support::{DAY, MIN, setup};

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
    assert_eq!(s.job("01-due").await.state, "done");
    assert_eq!(s.job("02-later").await.state, "queued");
    assert_eq!(s.tick(t0 + 1).await.claimed, 0);
    assert_eq!(s.tick(t0 + MIN).await.done, 1);
}

#[tokio::test]
async fn failures_retry_after_1_5_and_30_minutes_then_fail_with_last_error() {
    let s = setup().await;
    let t0 = s.now();
    s.queue("flaky", "demo.flaky", t0).await;

    assert_eq!(s.tick(t0).await.retried, 1);
    let j = s.job("flaky").await;
    assert_eq!(
        (j.state.as_str(), j.attempts, j.run_at),
        ("queued", 1, t0 + MIN)
    );
    assert_eq!(j.last_error.as_deref(), Some("platform: attempt 1 failed"));

    let t1 = t0 + MIN;
    assert_eq!(s.tick(t1).await.retried, 1);
    assert_eq!(
        s.job("flaky").await.run_at,
        t1 + 5 * MIN,
        "second step is 5 min"
    );
    assert_eq!(s.tick(t1 + 4 * MIN).await.claimed, 0, "not due yet");

    let t2 = t1 + 5 * MIN;
    assert_eq!(s.tick(t2).await.retried, 1);
    assert_eq!(
        s.job("flaky").await.run_at,
        t2 + 30 * MIN,
        "third step is 30 min"
    );

    let r = s.tick(t2 + 30 * MIN).await;
    assert_eq!((r.claimed, r.failed), (1, 1));
    let j = s.job("flaky").await;
    assert_eq!(
        (j.state.as_str(), j.attempts),
        ("failed", 4),
        "3 retries = 4 runs"
    );
    assert_eq!(j.last_error.as_deref(), Some("platform: attempt 4 failed"));
    assert_eq!(s.tick(t2 + 300 * MIN).await.claimed, 0);
}

#[tokio::test]
async fn a_row_of_an_unknown_kind_fails_after_retries_and_never_spins() {
    let s = setup().await;
    let mut at = s.now();
    s.queue("ghost", "demo.unregistered", at).await;
    for step in [MIN, 5 * MIN, 30 * MIN] {
        assert_eq!(s.tick(at).await.retried, 1);
        at += step;
    }
    assert_eq!(s.tick(at).await.failed, 1);
    let j = s.job("ghost").await;
    assert_eq!((j.state.as_str(), j.attempts), ("failed", 4));
    assert!(
        j.last_error
            .unwrap()
            .contains("no job kind demo.unregistered")
    );
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
    let j = s.job("orphan-retry").await;
    assert_eq!((j.state.as_str(), j.attempts), ("done", 3));
    let j = s.job("orphan-spent").await;
    assert_eq!((j.state.as_str(), j.attempts), ("failed", 4));
    assert_eq!(j.last_error.as_deref(), Some("lock expired"));
    assert_eq!(s.tick(now + 60 * MIN).await.claimed, 0);
}

#[tokio::test]
async fn a_live_lock_is_not_reclaimed_until_it_expires() {
    let s = setup().await;
    let now = s.now();
    s.orphan("busy", 1).await;
    s.db()
        .execute(harpseal::Statement::new("UPDATE job SET locked_until = ?").bind(now + 15 * MIN))
        .await
        .unwrap();
    assert_eq!(s.tick(now).await.claimed, 0);
    assert_eq!(
        s.tick(now + 15 * MIN).await.claimed,
        0,
        "the lock holds to its end"
    );
    assert_eq!(s.job("busy").await.state, "running");
    assert_eq!(
        s.tick(now + 15 * MIN + 1).await.done,
        1,
        "then it is re-claimed"
    );
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
    let once = s
        .count("SELECT count(*) FROM job WHERE state = 'done' AND attempts = 1")
        .await;
    assert_eq!(once, 30);
}

#[tokio::test]
async fn a_panicking_handler_is_a_failed_run_and_is_retried() {
    let s = setup().await;
    let now = s.now();
    s.queue("boom", "demo.panic", now).await;
    s.queue("after", "demo.ok", now).await;
    let r = s.tick(now).await;
    assert_eq!(
        (r.claimed, r.retried, r.done),
        (2, 1, 1),
        "the next Job still ran"
    );
    let j = s.job("boom").await;
    assert_eq!((j.state.as_str(), j.run_at), ("queued", now + MIN));
    assert_eq!(
        j.last_error.as_deref(),
        Some("platform: job handler panicked: demo.panic handler panicked")
    );
}

#[tokio::test]
async fn a_handler_writes_progress_while_it_holds_the_lock() {
    let s = setup().await;
    let now = s.now();
    s.queue("chunks", "demo.progress", now).await;
    s.tick(now).await;
    let j = s.job("chunks").await;
    assert_eq!(j.progress.as_deref(), Some("chunk 2 of 3"));
    assert_eq!(
        j.last_error.as_deref(),
        Some("platform: stopped after chunk 2")
    );
}

#[tokio::test]
async fn done_rows_are_pruned_after_30_days_and_failed_after_90() {
    let s = setup().await;
    let t0 = s.now();
    s.queue("done-job", "demo.ok", t0).await;
    s.queue("failed-job", "demo.flaky", t0).await;
    s.db()
        .execute(harpseal::Statement::new(
            "UPDATE job SET max_attempts = 0 WHERE id = 'failed-job'",
        ))
        .await
        .unwrap();
    let r = s.tick(t0).await;
    assert_eq!((r.done, r.failed, r.jobs_pruned), (1, 1, 0));

    assert_eq!(
        s.tick(t0 + 30 * DAY).await.jobs_pruned,
        0,
        "30 days exactly: kept"
    );
    assert_eq!(s.tick(t0 + 30 * DAY + 1).await.jobs_pruned, 1);
    assert_eq!(
        s.job("failed-job").await.state,
        "failed",
        "failed rows stay longer"
    );
    assert_eq!(s.tick(t0 + 90 * DAY).await.jobs_pruned, 0);
    assert_eq!(s.tick(t0 + 90 * DAY + 1).await.jobs_pruned, 1);
    assert_eq!(s.count("SELECT count(*) FROM job").await, 0);
}

/// The shared entry both runtimes call (native timer, Workers
/// `scheduled`): one `Deployment::tick` claims, runs, retries and prunes,
/// with the ManualClock as the only time source.
#[tokio::test]
async fn one_tick_claims_runs_retries_and_prunes() {
    let s = setup().await;
    let t0 = s.now();
    s.queue("old", "demo.ok", t0).await;
    s.tick(t0).await;
    s.h.clock().advance_ms(31 * DAY);
    let now = s.now();
    s.queue("fresh", "demo.ok", now).await;
    s.queue("flaky", "demo.flaky", now).await;
    s.orphan("orphan", 1).await;

    let r = s.d.tick(s.d.services().clock.now()).await.unwrap();
    assert_eq!(
        (r.claimed, r.done, r.retried, r.failed, r.jobs_pruned),
        (3, 2, 1, 0, 1)
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        r.to_string(),
        "scheduled 0, claimed 3, done 2, retried 1, failed 0, pruned 1 jobs, 0 sessions, \
         0 webhook events and 0 roster files"
    );
    assert_eq!(s.job("orphan").await.state, "done");
    assert_eq!(s.job("flaky").await.run_at, now + MIN);
}

#[tokio::test]
async fn a_prune_backlog_drains_500_rows_per_state_per_tick() {
    let s = setup().await;
    s.db()
        .execute(harpseal::Statement::new(
            "INSERT INTO job (id, kind, run_at, state, created_at, finished_at) \
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1200) \
             SELECT printf('old-%04d', i), 'demo.ok', 0, 'done', 0, 0 FROM n",
        ))
        .await
        .unwrap();
    let now = s.now();
    let pruned: Vec<u64> = [
        s.tick(now).await,
        s.tick(now + MIN).await,
        s.tick(now + 2 * MIN).await,
        s.tick(now + 3 * MIN).await,
    ]
    .iter()
    .map(|r| r.jobs_pruned)
    .collect();
    assert_eq!(pruned, [500, 500, 200, 0], "three ticks drain 1200 rows");
    assert_eq!(s.count("SELECT count(*) FROM job").await, 0);
}

#[tokio::test]
async fn failed_rows_are_kept_as_long_as_done_rows_when_job_days_exceeds_90() {
    let h = harpseal_testing::Harness::new();
    let toml = format!(
        "{}\n[retention]\njob_days = 120\n",
        harpseal_testing::TEST_CONFIG
    );
    let config = harpseal::DeploymentConfig::from_toml(&toml).unwrap();
    let d = h
        .builder()
        .config(config)
        .module(jobs_support::Demo {
            runs: Default::default(),
        })
        .build()
        .unwrap();
    d.migrate().await.unwrap();
    let t0 = h.clock().now_ms();
    for (id, state) in [("done-1", "done"), ("failed-1", "failed")] {
        h.db()
            .execute(
                harpseal::Statement::new(
                    "INSERT INTO job (id, kind, run_at, state, created_at, finished_at) \
                     VALUES (?, 'demo.ok', ?, ?, ?, ?)",
                )
                .bind(id)
                .bind(t0)
                .bind(state)
                .bind(t0)
                .bind(t0),
            )
            .await
            .unwrap();
    }
    let tick = |at: i64| d.tick(Timestamp(at));
    assert_eq!(
        tick(t0 + 91 * DAY).await.unwrap().jobs_pruned,
        0,
        "not at 90 days"
    );
    assert_eq!(tick(t0 + 120 * DAY).await.unwrap().jobs_pruned, 0);
    assert_eq!(
        tick(t0 + 120 * DAY + 1).await.unwrap().jobs_pruned,
        2,
        "both at 120"
    );
}

#[tokio::test]
async fn a_claim_clears_the_previous_attempts_progress() {
    let s = setup().await;
    let now = s.now();
    s.queue("noted", "demo.flaky", now).await;
    s.db()
        .execute(harpseal::Statement::new(
            "UPDATE job SET progress = 'from the last attempt' WHERE id = 'noted'",
        ))
        .await
        .unwrap();
    assert_eq!(s.tick(now).await.retried, 1);
    assert_eq!(s.job("noted").await.progress, None);
}
