//! A Module notifies chosen Students (#73): chat on a live Link, metered
//! and recorded like a Kernel Push, else email, else skipped; the same key
//! never sends twice.

mod notify_support;

use harpseal::{NotifyOutcome, NotifySkip, StudentId};
use harpseal_channel_fake::{FakeChannel, FakeProfile};
use notify_support::Setup;

fn outcomes(s: &Setup) -> Vec<(Option<String>, NotifyOutcome, bool)> {
    s.last()
        .students
        .into_iter()
        .map(|n| (n.student.map(|StudentId(id)| id), n.outcome, n.again))
        .collect()
}

#[tokio::test]
async fn linked_students_get_chat_others_email_or_nothing() {
    let s = Setup::new(FakeProfile::LineLike).await;
    s.student(1, Some("s1@seal-bay.invalid"), Some("active"))
        .await;
    s.student(2, Some("s2@seal-bay.invalid"), None).await;
    s.student(3, None, None).await;
    s.student(4, Some("s4@seal-bay.invalid"), Some("unreachable"))
        .await;
    s.student(5, Some("s5@seal-bay.invalid"), Some("active"))
        .await;
    s.student(6, Some("s6@seal-bay.invalid"), Some("revoked"))
        .await;
    s.run("UPDATE student SET status = 'left' WHERE id = 's6'")
        .await;
    // Student 5's account blocked the bot: the platform refuses the Push.
    s.fake().set_unreachable(&FakeChannel::account("u5"), true);

    let codes = [
        "68010001", "68010002", "68010003", "68010004", "68010005", "68010006", "68019999",
    ];
    let t = s.notify("fees:w45", &codes).await;
    assert_eq!(t.done, 1, "{:?}", t.errors);
    let o = |id: &str, outcome| (Some(id.to_string()), outcome, false);
    assert_eq!(
        outcomes(&s),
        vec![
            o("s1", NotifyOutcome::Sent),
            o("s2", NotifyOutcome::Emailed),
            o("s3", NotifyOutcome::Skipped(NotifySkip::NoAddress)),
            o("s4", NotifyOutcome::Emailed),
            o("s5", NotifyOutcome::Emailed),
            o("s6", NotifyOutcome::Skipped(NotifySkip::Left)),
            (None, NotifyOutcome::Skipped(NotifySkip::Unknown), false),
        ]
    );
    let r = s.last();
    assert_eq!((r.sent, r.emailed, r.skipped, r.failed), (1, 3, 3, 0));

    // Chat: both Thai Messages, as one metered Push request.
    let seen = s.fake().seen_by(&FakeChannel::account("u1"));
    assert_eq!(seen.len(), 2);
    let emails = s.h.emails();
    let to: Vec<&str> = emails.iter().map(|e| e.to.as_str()).collect();
    assert_eq!(
        to,
        [
            "s2@seal-bay.invalid",
            "s4@seal-bay.invalid",
            "s5@seal-bay.invalid"
        ]
    );
    assert_eq!(emails[0].subject, "แจ้งเตือนค่าธรรมเนียม");
    assert_eq!(emails[0].text, "ชำระภายในวันศุกร์");
    // The refused Push marked student 5's Link unreachable, as a Kernel
    // send does.
    let link = s
        .app
        .count("SELECT count(*) FROM link WHERE id = 'l5' AND status = 'unreachable'");
    assert_eq!(link.await, 1);

    // Outbound Message rows and usage exactly like a Kernel Push.
    let rows = s
        .app
        .query(
            "SELECT module, batch_key, msg_index, units, push, metering, kind \
             FROM outbound_message ORDER BY msg_index",
        )
        .await;
    assert_eq!(rows.len(), 2);
    for i in 0..2 {
        let text = |c: &str| rows.get(i, c).and_then(|v| v.as_text()).map(str::to_string);
        let int = |c: &str| rows.get(i, c).and_then(|v| v.as_i64());
        assert_eq!(text("module").as_deref(), Some("probe"));
        assert_eq!(text("batch_key").as_deref(), Some("notify:fees:w45:s1"));
        assert_eq!(int("msg_index"), Some(i as i64));
        assert_eq!(int("push"), Some(1));
        assert_eq!(text("kind").as_deref(), Some("text"));
    }
    assert_eq!(
        s.app.count("SELECT sum(units) FROM outbound_message").await,
        1,
        "one Push request to one recipient is one unit"
    );
    assert_eq!(s.app.count("SELECT sum(metered) FROM usage_daily").await, 1);
}

#[tokio::test]
async fn a_retry_with_the_same_key_sends_nothing_new() {
    let s = Setup::new(FakeProfile::LineLike).await;
    s.student(1, Some("s1@seal-bay.invalid"), Some("active"))
        .await;
    s.student(2, Some("s2@seal-bay.invalid"), None).await;
    let codes = ["68010001", "68010002"];
    s.notify("fees:w45", &codes).await;
    let calls = s.fake().calls().len();
    let rows = s.app.count("SELECT count(*) FROM outbound_message").await;

    s.notify("fees:w45", &codes).await;
    let again = |id: &str, outcome| (Some(id.to_string()), outcome, true);
    assert_eq!(
        outcomes(&s),
        vec![
            again("s1", NotifyOutcome::Sent),
            again("s2", NotifyOutcome::Emailed)
        ]
    );
    assert_eq!(s.fake().calls().len(), calls, "no platform call");
    assert_eq!(s.h.emails().len(), 1, "no second email");
    assert_eq!(
        s.app.count("SELECT count(*) FROM outbound_message").await,
        rows
    );
    assert_eq!(s.app.count("SELECT sum(metered) FROM usage_daily").await, 1);

    // A new key is a new notice.
    s.notify("fees:w46", &codes).await;
    assert_eq!(s.fake().seen_by(&FakeChannel::account("u1")).len(), 4);
    assert_eq!(s.h.emails().len(), 2);
}

#[tokio::test]
async fn a_send_cut_off_part_way_resumes_without_a_repeat() {
    // Telegram-like: one request per Message, no retry keys, so only the
    // rows keep the first Message from going twice.
    let s = Setup::new(FakeProfile::TelegramLike).await;
    s.student(1, None, Some("active")).await;
    s.student(2, None, Some("active")).await;
    s.fake().fail_next_batch_at(1);
    let t = s.notify("fees:w45", &["68010001", "68010002"]).await;
    assert_eq!(t.retried, 1, "a rate limit fails the run: {t:?}");
    assert!(s.reports.lock().unwrap().is_empty());
    assert_eq!(s.fake().seen_by(&FakeChannel::account("u1")).len(), 1);

    s.h.clock().advance_minutes(1);
    let t = s.tick().await;
    assert_eq!(t.done, 1, "{:?}", t.errors);
    let r = s.last();
    assert_eq!((r.sent, r.failed), (2, 0));
    for u in ["u1", "u2"] {
        let seen = s.fake().sent_to(&FakeChannel::account(u));
        assert_eq!(seen.len(), 2, "{u}: each Message once");
    }
    assert_eq!(
        s.app.count("SELECT count(*) FROM outbound_message").await,
        4
    );
}

#[tokio::test]
async fn a_failed_email_is_retried_and_then_never_sent_again() {
    let s = Setup::new(FakeProfile::LineLike).await;
    s.student(2, Some("s2@seal-bay.invalid"), None).await;
    s.h.email().fail_for("s2@seal-bay.invalid", true);
    s.notify("fees:w45", &["68010002"]).await;
    assert_eq!(
        outcomes(&s),
        vec![(Some("s2".into()), NotifyOutcome::Failed("email"), false)]
    );
    assert_eq!(s.last().failed, 1);

    s.h.email().fail_for("s2@seal-bay.invalid", false);
    s.notify("fees:w45", &["68010002"]).await;
    assert_eq!(s.last().emailed, 1);
    assert!(!s.last().students[0].again);
    s.notify("fees:w45", &["68010002"]).await;
    assert!(s.last().students[0].again);
    assert_eq!(s.h.email().to("s2@seal-bay.invalid").len(), 1);
}

#[tokio::test]
async fn a_platform_failure_is_reported_and_retried_by_key() {
    let s = Setup::new(FakeProfile::LineLike).await;
    s.student(1, Some("s1@seal-bay.invalid"), Some("active"))
        .await;
    s.fake().fail_next_batch_at_with(0, "bad account");
    s.notify("fees:w45", &["68010001"]).await;
    assert_eq!(
        s.last().students[0].outcome,
        NotifyOutcome::Failed("platform")
    );
    assert!(
        s.h.emails().is_empty(),
        "a platform error is not unreachable"
    );
    s.notify("fees:w45", &["68010001"]).await;
    assert_eq!(s.last().students[0].outcome, NotifyOutcome::Sent);
    assert_eq!(s.fake().seen_by(&FakeChannel::account("u1")).len(), 2);
}

#[tokio::test]
async fn a_list_over_the_limit_or_a_bad_notice_sends_nothing() {
    let s = Setup::new(FakeProfile::LineLike).await;
    s.student(1, None, Some("active")).await;
    let many: Vec<String> = (0..=harpseal::MAX_NOTIFY)
        .map(|n| format!("6802{n:04}"))
        .collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let t = s.notify("fees:w45", &many).await;
    assert_eq!(t.retried, 1);
    let why = "SELECT count(*) FROM job WHERE last_error LIKE '%MAX_NOTIFY%'";
    assert_eq!(s.app.count(why).await, 1);
    let t = s.notify("", &["68010001"]).await;
    assert_eq!(t.retried, 1);
    assert!(s.fake().calls().is_empty());
}
