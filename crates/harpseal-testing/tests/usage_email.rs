//! Quota emails (#26): Job `kernel.usage.watch` emails every active
//! Administrator once per calendar month per Channel per level, claims
//! each notification once however many runs race, retries addresses that
//! failed, and starts over in a new local month.

mod usage_support;

use harpseal::Timestamp;
use harpseal_testing::{Harness, START_MS};
use usage_support::*;

const SUBJECT_WARN: &str = "[Seal Bay University (usage)] fake ใช้ไป 80% ของโควตาเดือน 2026-11";

/// Two active Administrators, a disabled one, and an Officer.
async fn people(h: &Harness) {
    staff(h, "admin1", &["Administrator"]).await;
    staff(h, "admin2", &["Administrator", "Officer"]).await;
    let gone = staff(h, "gone", &["Administrator"]).await;
    run(
        h,
        &format!("UPDATE staff SET status = 'disabled' WHERE id = '{gone}'"),
    )
    .await;
    staff(h, "officer", &["Officer"]).await;
}

fn recipients(h: &Harness) -> Vec<String> {
    let mut to: Vec<String> = h.emails().into_iter().map(|e| e.to).collect();
    to.sort();
    to
}

#[tokio::test]
async fn one_email_per_administrator_at_warn_then_at_over() {
    let (h, app) = boot().await;
    people(&h).await;
    use_percent(&h, 79).await;
    watch(&app).await;
    assert!(h.emails().is_empty());

    use_percent(&h, 80).await;
    watch(&app).await;
    assert_eq!(recipients(&h), [email_of("admin1"), email_of("admin2")]);
    let e = h.email().last().unwrap();
    assert_eq!(e.subject, SUBJECT_WARN);
    assert!(
        e.text
            .contains("ใช้ Push ที่คิดค่าบริการไปแล้ว 240 จาก 300 ครั้ง (80%) ในเดือน 2026-11")
    );
    assert!(e.text.contains("/admin/usage"));
    watch(&app).await;
    use_percent(&h, 95).await;
    watch(&app).await;
    assert_eq!(h.emails().len(), 2);

    use_percent(&h, 100).await;
    watch(&app).await;
    assert_eq!(h.emails().len(), 4);
    let over = h.email().to(&email_of("admin1"));
    assert_eq!(
        over[1].subject,
        "[Seal Bay University (usage)] fake ถึงโควตาเดือน 2026-11 แล้ว (100%)"
    );
    use_percent(&h, 140).await;
    watch(&app).await;
    assert_eq!(h.emails().len(), 4);

    // One audit row per notification, actor system.
    let rows = app
        .query(
            "SELECT actor_kind, target_id, json_extract(after, '$.level') AS level, \
             json_extract(after, '$.used') AS used, json_extract(after, '$.cap') AS cap, \
             json_extract(after, '$.recipients') AS n FROM audit_log \
             WHERE action = 'usage.notify' ORDER BY at, level DESC",
        )
        .await;
    assert_eq!(rows.len(), 2);
    let text = |i, c| {
        rows.get(i, c)
            .and_then(|v| v.as_text())
            .unwrap()
            .to_string()
    };
    let int = |i, c| rows.get(i, c).and_then(|v| v.as_i64()).unwrap();
    assert_eq!(
        (text(0, "actor_kind"), text(0, "target_id")),
        ("system".into(), "fake".into())
    );
    assert_eq!(
        (text(0, "level"), int(0, "used"), int(0, "cap"), int(0, "n")),
        ("warn".into(), 240, 300, 2)
    );
    assert_eq!((text(1, "level"), int(1, "used")), ("over".into(), 300));
}

#[tokio::test]
async fn straight_past_100_percent_sends_the_over_email_only() {
    let (h, app) = boot().await;
    people(&h).await;
    use_percent(&h, 120).await;
    watch(&app).await;
    watch(&app).await;
    assert_eq!(h.emails().len(), 2);
    assert!(h.emails().iter().all(|e| e.subject.contains("ถึงโควตา")));
}

#[tokio::test]
async fn runs_at_the_same_time_send_once() {
    let (h, app) = boot().await;
    people(&h).await;
    use_percent(&h, 90).await;
    tokio::join!(watch(&app), watch(&app), watch(&app));
    assert_eq!(h.emails().len(), 2);
    // Two ticks in the same scheduled minute: one Job, one send.
    use_percent(&h, 100).await;
    let at = Timestamp(START_MS + 10 * 60_000);
    let d = app.deployment();
    let (a, b) = tokio::join!(d.tick(at), d.tick(at));
    assert_eq!(a.unwrap().scheduled + b.unwrap().scheduled, 1);
    assert_eq!(h.emails().len(), 4);
    assert_eq!(
        int(
            &h,
            "SELECT count(*) FROM audit_log WHERE action = 'usage.notify'"
        )
        .await,
        2
    );
}

#[tokio::test]
async fn the_scheduled_job_runs_every_ten_minutes() {
    let (h, app) = boot().await;
    people(&h).await;
    use_percent(&h, 80).await;
    let d = app.deployment();
    // 00:05 UTC: not a scheduled minute.
    let r = d.tick(Timestamp(START_MS + 5 * 60_000)).await.unwrap();
    assert_eq!(r.scheduled, 0);
    assert!(h.emails().is_empty());
    let r = d.tick(Timestamp(START_MS + 10 * 60_000)).await.unwrap();
    assert_eq!((r.scheduled, r.done, r.failed), (1, 1, 0));
    assert_eq!(h.emails().len(), 2);
}

#[tokio::test]
async fn a_failing_address_is_retried_on_the_next_run_only() {
    let (h, app) = boot().await;
    people(&h).await;
    h.email().fail_for(&email_of("admin2"), true);
    use_percent(&h, 80).await;
    watch(&app).await; // never fails the Job
    assert_eq!(recipients(&h), [email_of("admin1")]);
    watch(&app).await;
    assert_eq!(h.emails().len(), 1);
    h.email().fail_for(&email_of("admin2"), false);
    watch(&app).await;
    assert_eq!(recipients(&h), [email_of("admin1"), email_of("admin2")]);
    watch(&app).await;
    assert_eq!(h.emails().len(), 2);
}

#[tokio::test]
async fn a_new_local_month_starts_over() {
    let (h, app) = boot().await;
    people(&h).await;
    use_percent(&h, 100).await;
    watch(&app).await;
    assert_eq!(h.emails().len(), 2);
    // 00:30 on 1 December in +07:00: November's usage no longer counts.
    h.clock().set_ms(DEC_1_0030);
    watch(&app).await;
    assert_eq!(h.emails().len(), 2);
    seed(&h, "fake", "2026-12-01", 240, 0, 0).await;
    watch(&app).await;
    assert_eq!(h.emails().len(), 4);
    let e = h.email().last().unwrap();
    assert_eq!(
        e.subject,
        "[Seal Bay University (usage)] fake ใช้ไป 80% ของโควตาเดือน 2026-12"
    );
    watch(&app).await;
    assert_eq!(h.emails().len(), 4);
}

#[tokio::test]
async fn a_channel_without_a_quota_never_emails() {
    let (h, app) = boot().await;
    people(&h).await;
    seed(&h, "fake2", TODAY, 1_000_000, 0, 0).await;
    watch(&app).await;
    assert!(h.emails().is_empty());
    assert_eq!(int(&h, "SELECT count(*) FROM setting").await, 0);
}
