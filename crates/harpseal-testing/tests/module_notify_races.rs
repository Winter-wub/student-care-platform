//! A Notice (#73) under races and cut-off runs: two calls with one key at
//! once, two Modules with the same key, a run cut off after its chat or
//! its email, and a call over its request budget.

mod notify_support;

use harpseal::{NotifyDefer, NotifyOutcome};
use harpseal_channel_fake::{FakeChannel, FakeProfile};
use notify_support::Setup;
use serde_json::json;

fn chats(s: &Setup, u: &str) -> usize {
    s.fake().sent_to(&FakeChannel::account(u)).len()
}

/// Rewinds every claim of the probe's `key` to a run cut off before its
/// last write, its lease long gone.
async fn cut_off(s: &Setup, key: &str, via: &str) {
    s.run(&format!(
        "UPDATE notify_delivery SET status = 'sending', via = '{via}', claimed_at = 0 \
         WHERE notify_key = 'probe:{key}'"
    ))
    .await;
}

#[tokio::test]
async fn two_calls_with_one_key_at_once_send_each_student_once() {
    let s = Setup::new(FakeProfile::TelegramLike).await;
    s.student(1, None, Some("active")).await;
    s.student(2, Some("s2@seal-bay.invalid"), None).await;
    s.student(3, None, Some("active")).await;
    let path = "/probe/notify?key=w45&codes=68010001,68010002,68010003";
    let (a, b) = tokio::join!(s.app.get(path), s.app.get(path));
    assert!(
        a.body.starts_with("Ok") && b.body.starts_with("Ok"),
        "{a:?} {b:?}"
    );
    assert_eq!(chats(&s, "u1"), 2, "one batch of 2 Messages");
    assert_eq!(chats(&s, "u3"), 2);
    assert_eq!(s.h.email().to("s2@seal-bay.invalid").len(), 1);
    // Each Student was sent by one call; the other saw it done or busy.
    let reports = s.reports.lock().unwrap().clone();
    for i in 0..3 {
        let fresh = reports
            .iter()
            .filter(|r| {
                let n = &r.students[i];
                !n.again && matches!(n.outcome, NotifyOutcome::Sent | NotifyOutcome::Emailed)
            })
            .count();
        assert_eq!(fresh, 1, "student {i}: {reports:?}");
    }
}

#[tokio::test]
async fn two_modules_with_the_same_key_each_send() {
    let s = Setup::new(FakeProfile::LineLike).await;
    s.student(1, Some("s1@seal-bay.invalid"), Some("active"))
        .await;
    s.student(2, Some("s2@seal-bay.invalid"), None).await;
    let codes = "68010001,68010002";
    for module in ["fees", "library"] {
        s.job(json!({ "module": module, "key": "2026-11-09", "codes": codes }))
            .await;
        let r = s.last();
        assert_eq!((r.sent, r.emailed), (1, 1), "{module}");
        assert!(r.students.iter().all(|n| !n.again), "{module}");
    }
    assert_eq!(chats(&s, "u1"), 4);
    assert_eq!(s.h.email().to("s2@seal-bay.invalid").len(), 2);
    let keys = s
        .app
        .count("SELECT count(DISTINCT batch_key) FROM outbound_message")
        .await;
    assert_eq!(keys, 2, "notify:fees:… and notify:library:…");
}

#[tokio::test]
async fn a_chat_on_record_is_not_emailed_after_its_link_is_gone() {
    let s = Setup::new(FakeProfile::LineLike).await;
    s.student(1, Some("s1@seal-bay.invalid"), Some("active"))
        .await;
    s.notify("w45", &["68010001"]).await;
    assert_eq!(chats(&s, "u1"), 2);
    // Cut off before `done`; then the Student unlinks.
    cut_off(&s, "w45", "chat").await;
    s.run("UPDATE link SET status = 'revoked' WHERE id = 'l1'")
        .await;
    s.notify("w45", &["68010001"]).await;
    assert_eq!(s.last().students[0].outcome, NotifyOutcome::Sent);
    assert!(
        s.h.emails().is_empty(),
        "the chat went out: no email on top"
    );
    assert_eq!(chats(&s, "u1"), 2);
}

#[tokio::test]
async fn an_email_cut_off_is_resent_keyed_only_when_the_sender_dedupes() {
    let s = Setup::new(FakeProfile::LineLike).await;
    s.student(2, Some("s2@seal-bay.invalid"), None).await;
    s.notify("w45", &["68010002"]).await;
    assert_eq!(s.h.email().keys(), ["notify:probe:w45:s2"]);

    // SMTP-like (no de-duplication): at most once, reported as emailed.
    cut_off(&s, "w45", "email").await;
    s.notify("w45", &["68010002"]).await;
    assert_eq!(s.last().students[0].outcome, NotifyOutcome::Emailed);
    assert_eq!(s.h.email().to("s2@seal-bay.invalid").len(), 1);

    // Resend-like: resent with the same key; the provider keeps one.
    s.h.email().set_dedupes(true);
    cut_off(&s, "w45", "email").await;
    s.notify("w45", &["68010002"]).await;
    assert_eq!(s.last().students[0].outcome, NotifyOutcome::Emailed);
    assert_eq!(s.h.email().to("s2@seal-bay.invalid").len(), 1);
    let keys = s.h.email().keys();
    assert_eq!(keys, ["notify:probe:w45:s2"], "the repeat was dropped");

    // A claim still within its lease is another call's: not touched.
    s.run(
        "UPDATE notify_delivery SET status = 'sending', claimed_at = 1794189600000 \
         WHERE notify_key = 'probe:w45'",
    )
    .await;
    s.h.clock().set_ms(1_794_189_600_000 + 60_000);
    s.notify("w45", &["68010002"]).await;
    let busy = NotifyOutcome::Deferred(NotifyDefer::InProgress);
    assert_eq!(s.last().students[0].outcome, busy);
}

#[tokio::test]
async fn a_call_over_its_budget_defers_the_rest() {
    // Telegram-like: 2 Messages = 2 requests per Student with one Link.
    let s = Setup::new(FakeProfile::TelegramLike).await;
    for n in 1..=3 {
        s.student(n, None, Some("active")).await;
    }
    let codes = "68010001,68010002,68010003";
    s.job(json!({ "key": "w45", "codes": codes, "budget": 5 }))
        .await;
    let r = s.last();
    assert_eq!((r.sent, r.deferred), (2, 1), "{r:?}");
    let budget = NotifyOutcome::Deferred(NotifyDefer::Budget);
    assert_eq!(r.students[2].outcome, budget);
    assert_eq!(chats(&s, "u3"), 0);
    // The next call with the key sends the rest only.
    s.job(json!({ "key": "w45", "codes": codes, "budget": 5 }))
        .await;
    let r = s.last();
    assert_eq!((r.sent, r.deferred), (3, 0));
    assert_eq!((chats(&s, "u1"), chats(&s, "u3")), (2, 2));
}

#[tokio::test]
async fn old_rows_are_pruned_after_90_days() {
    let s = Setup::new(FakeProfile::LineLike).await;
    s.student(2, Some("s2@seal-bay.invalid"), None).await;
    s.notify("w45", &["68010002"]).await;
    assert_eq!(s.app.count("SELECT count(*) FROM notify_delivery").await, 1);
    s.h.clock().advance_minutes(91 * 24 * 60);
    s.notify("other", &["68019999"]).await;
    assert_eq!(s.app.count("SELECT count(*) FROM notify_delivery").await, 0);
}

#[tokio::test]
async fn a_restored_backup_keeps_what_was_sent() {
    use harpseal::backup::{self, LoadOptions, MemoryBackup};
    let s = Setup::new(FakeProfile::LineLike).await;
    s.student(2, Some("s2@seal-bay.invalid"), None).await;
    s.notify("w45", &["68010002"]).await;
    let mut b = MemoryBackup::new();
    backup::dump(s.app.deployment(), &mut b).await.unwrap();
    assert!(b.entries.contains_key("tables/notify_delivery.ndjson"));

    let fresh = harpseal_testing::Harness::new();
    let reports = notify_support::Reports::default();
    let d = fresh
        .builder()
        .module(notify_support::Probe(reports.clone()))
        .build()
        .unwrap();
    backup::load(&d, &b, &LoadOptions::new()).await.unwrap();
    let rows = d
        .services()
        .db
        .query(harpseal::Statement::new(
            "SELECT count(*) AS n FROM notify_delivery WHERE status = 'done'",
        ))
        .await
        .unwrap();
    let n = rows.get(0, "n").and_then(|v| v.as_i64());
    assert_eq!(n, Some(1), "the done row survives the restore");
}
