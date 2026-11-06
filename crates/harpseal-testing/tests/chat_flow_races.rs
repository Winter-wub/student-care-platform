//! The chat flow under concurrency (#19 review): the OTP attempt cap is
//! claimed atomically, two accounts racing for one student id, and two
//! Inbounds of one account racing through the student-id step. The
//! harness's SQLite runs each statement on tokio's blocking pool, so two
//! webhooks joined here really interleave between statements.

mod support;

use harpseal::{Inbound, Message, ReplyToken, Statement, Timestamp};
use harpseal_channel_fake::{FakeChannel, FakeProfile};
use harpseal_testing::{Harness, TestApp};
use support::{Chat, boot, text_of};

fn typed(who: &str, text: &str) -> Inbound {
    Inbound::Text {
        from: FakeChannel::account(who),
        text: text.into(),
        reply: Some(ReplyToken {
            token: format!("{who}-{text}"),
            expires_at: Timestamp(harpseal_testing::START_MS + 60_000),
        }),
    }
}

/// `u1` waiting for its code; returns the right code and a wrong one.
async fn waiting_for_code<'a>(h: &'a Harness, app: &'a TestApp) -> (Chat<'a>, String, String) {
    let mut c = Chat::new(h, app, "u1");
    c.follow().await;
    c.press("kernel.consent.accept").await;
    c.text("68010001").await;
    let code = c.code();
    let wrong = if code == "000000" { "111111" } else { "000000" }.to_string();
    (c, code, wrong)
}

#[tokio::test]
async fn a_used_up_challenge_refuses_even_the_right_code() {
    let (h, app) = boot(FakeProfile::Full).await;
    let (mut c, code, _) = waiting_for_code(&h, &app).await;
    // Three wrong answers already claimed (by concurrent Inbounds whose
    // lockout has not landed yet).
    h.db()
        .execute(Statement::new(
            "UPDATE verification_challenge SET attempts = 3",
        ))
        .await
        .unwrap();
    let seen = c.text(&code).await;
    assert!(text_of(&seen[0]).starts_with("ใส่รหัสผิดเกินกำหนด"), "{seen:?}");
    assert_eq!(app.count("SELECT count(*) FROM link").await, 0);
}

#[tokio::test]
async fn the_cap_holds_with_answers_submitted_at_once() {
    let (h, app) = boot(FakeProfile::Full).await;
    let (mut c, code, wrong) = waiting_for_code(&h, &app).await;
    c.text(&wrong).await;
    c.text(&wrong).await;
    // The third wrong answer and the right one arrive together.
    let (a, b) = tokio::join!(
        app.fake_webhook([typed("u1", &wrong)]),
        app.fake_webhook([typed("u1", &code)]),
    );
    assert_eq!((a.status.as_u16(), b.status.as_u16()), (200, 200));
    let attempts = app
        .count("SELECT attempts FROM verification_challenge")
        .await;
    assert!(attempts <= 3, "{attempts} attempts claimed");
    let linked = app.count("SELECT count(*) FROM link").await;
    let locked = app
        .count("SELECT count(*) FROM student WHERE id = 's1' AND verification_locked_until IS NOT NULL")
        .await;
    assert_eq!(
        linked + locked,
        1,
        "either the code won or the lock did, never both"
    );

    // Many wrong answers at once on a fresh challenge: still at most 3.
    let (h, app) = boot(FakeProfile::Full).await;
    let (_c, _, wrong) = waiting_for_code(&h, &app).await;
    let w = || app.fake_webhook([typed("u1", &wrong)]);
    tokio::join!(w(), w(), w(), w(), w());
    let attempts = app
        .count("SELECT attempts FROM verification_challenge")
        .await;
    assert_eq!(attempts, 3);
    assert_eq!(
        app.count("SELECT count(*) FROM security_event WHERE kind = 'otp.lockout'")
            .await,
        1
    );
}

#[tokio::test]
async fn the_account_that_loses_the_race_for_a_student_id_is_refused() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut a = Chat::new(&h, &app, "a");
    a.follow().await;
    a.press("kernel.consent.accept").await;
    a.text("68010001").await;
    let code_a = a.code();
    let mut b = Chat::new(&h, &app, "b");
    b.link("68010001").await;
    assert_eq!(app.count("SELECT count(*) FROM link").await, 1);

    let seen = a.text(&code_a).await;
    assert!(
        matches!(&seen[..], [Message::Text(t)] if t.starts_with("รหัสนักศึกษานี้เชื่อมกับบัญชีอื่น")),
        "{seen:?}"
    );
    let to_b = b.seen_since();
    assert!(text_of(to_b.last().unwrap()).starts_with("มีผู้พยายามเชื่อมรหัสนักศึกษาของคุณ"));
    assert_eq!(app.count("SELECT count(*) FROM link").await, 1);
    assert_eq!(
        app.count("SELECT count(*) FROM security_event WHERE kind = 'link.duplicate_attempt'")
            .await,
        1
    );
    let state = app
        .query("SELECT flow_state FROM channel_account WHERE external_id = 'a'")
        .await;
    assert_eq!(
        state.get(0, "flow_state").unwrap().as_text(),
        Some("awaiting_student_id")
    );
    assert_eq!(
        app.count("SELECT count(*) FROM security_event WHERE kind = 'inbound.failed'")
            .await,
        0
    );
}

#[tokio::test]
async fn two_student_ids_at_once_open_one_challenge() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.follow().await;
    c.press("kernel.consent.accept").await;
    let (a, b) = tokio::join!(
        app.fake_webhook([typed("u1", "68010001")]),
        app.fake_webhook([typed("u1", "68010001")]),
    );
    assert_eq!((a.status.as_u16(), b.status.as_u16()), (200, 200));
    assert_eq!(
        app.count("SELECT count(*) FROM verification_challenge")
            .await,
        1
    );
    assert_eq!(h.emails().len(), 1);
}
