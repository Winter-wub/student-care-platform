//! The chat flow's security branches (#19): a second account claiming a
//! linked student id, and the OTP lockout.

mod support;

use harpseal::Message;
use harpseal_channel_fake::FakeProfile;
use support::{Chat, boot, text_of};

#[tokio::test]
async fn a_duplicate_link_is_refused_and_the_old_account_warned() {
    let (h, app) = boot(FakeProfile::LineLike).await;
    let mut owner = Chat::new(&h, &app, "owner");
    owner.link("68010001").await;
    h.fake_channel().take_sent();

    let mut other = Chat::new(&h, &app, "intruder");
    other.follow().await;
    other.press("kernel.consent.accept").await;
    let seen = other.text("68010001").await;
    assert!(matches!(&seen[..], [Message::Text(t)] if t.starts_with("รหัสนักศึกษานี้เชื่อมกับบัญชีอื่น")));
    assert_eq!(h.emails().len(), 1, "no code sent for the duplicate");

    // The warning reached the owner as a metered Push.
    let to_owner: Vec<_> = h
        .fake_channel()
        .sent()
        .into_iter()
        .filter(|s| s.to == owner.who)
        .collect();
    assert_eq!(to_owner.len(), 1);
    assert!(text_of(&to_owner[0].message).starts_with("มีผู้พยายามเชื่อมรหัสนักศึกษาของคุณ"));
    assert_eq!(to_owner[0].metering, harpseal::Metering::Metered);

    let ev = app
        .query("SELECT kind, student_id, channel_account_id, details FROM security_event")
        .await;
    assert_eq!(ev.len(), 1);
    assert_eq!(
        ev.get(0, "kind").unwrap().as_text(),
        Some("link.duplicate_attempt")
    );
    assert_eq!(ev.get(0, "student_id").unwrap().as_text(), Some("s1"));
    let intruder = app
        .query("SELECT id FROM channel_account WHERE external_id = 'intruder'")
        .await;
    assert_eq!(ev.get(0, "channel_account_id"), intruder.get(0, "id"));
    assert_eq!(app.count("SELECT count(*) FROM link").await, 1);
}

#[tokio::test]
async fn the_third_wrong_code_locks_the_student() {
    let (h, app) = boot(FakeProfile::Full).await;
    // Linked once, then unlinked, so a second account may claim the id.
    let mut phone = Chat::new(&h, &app, "phone");
    phone.link("68010001").await;
    let mut other = Chat::new(&h, &app, "tablet");
    phone.press("kernel.link.unlink").await;
    phone.press("kernel.link.unlink_confirm").await;
    other.follow().await;
    other.press("kernel.consent.accept").await;
    other.text("68010001").await;
    let code = other.code();
    let wrong = if code == "000000" { "111111" } else { "000000" };

    let seen = other.text(wrong).await;
    assert_eq!(text_of(&seen[0]), "รหัสไม่ถูกต้อง เหลือโอกาสอีก 2 ครั้ง");
    let seen = other.text(wrong).await;
    assert_eq!(text_of(&seen[0]), "รหัสไม่ถูกต้อง เหลือโอกาสอีก 1 ครั้ง");
    let seen = other.text(wrong).await;
    assert_eq!(
        text_of(&seen[0]),
        "ใส่รหัสผิดเกินกำหนด ระงับการยืนยันตัวตนไว้ชั่วคราว ลองใหม่ได้ในอีก 30 นาที"
    );
    let locked = app
        .count("SELECT verification_locked_until FROM student WHERE id = 's1'")
        .await;
    assert_eq!(locked, harpseal_testing::START_MS + 30 * 60_000);
    assert_eq!(
        app.count("SELECT count(*) FROM security_event WHERE kind = 'otp.lockout'")
            .await,
        1
    );
    assert_eq!(
        app.count("SELECT count(*) FROM verification_challenge WHERE state = 'failed'")
            .await,
        1
    );

    // The right code no longer helps: back to the student-id step, locked.
    let seen = other.text(&code).await;
    assert!(
        text_of(&seen[0]).starts_with("ไม่พบ"),
        "read as a student id"
    );
    let seen = other.text("68010001").await;
    assert_eq!(
        text_of(&seen[0]),
        "ใส่รหัสผิดเกินกำหนด ระงับการยืนยันตัวตนไว้ชั่วคราว ลองใหม่ได้ในอีก 30 นาที"
    );
    h.clock().advance_minutes(31);
    other.text("68010001").await;
    assert_eq!(h.emails().len(), 3, "a code again once the lock is over");
}

#[tokio::test]
async fn a_lockout_pushes_a_notice_to_the_students_other_links() {
    let (h, app) = boot(FakeProfile::Full).await;
    // s1 holds an active Link on another (seeded) Channel account.
    for sql in [
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
         VALUES ('old', 'fake', 'old-phone', 1, 1)",
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, linked_at, updated_at) \
         VALUES ('l-old', 's1', 'old', 'fake-2', 'active', 'manual', 1, 1)",
    ] {
        h.db().execute(harpseal::Statement::new(sql)).await.unwrap();
    }
    let mut c = Chat::new(&h, &app, "new-phone");
    c.follow().await;
    c.press("kernel.consent.accept").await;
    c.text("68010001").await;
    let code = c.code();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..3 {
        c.text(wrong).await;
    }
    let old = Chat::new(&h, &app, "old-phone");
    let seen = old.seen_since();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(text_of(&seen[0]).starts_with("มีการใส่รหัสยืนยันของคุณผิดหลายครั้ง"));
}
