//! The claim refuses a Student whose email cannot be trusted with a code
//! (#71): none, not valid (the roster only warns about it), or shared with
//! another Student who is not left. Fictional Students of Seal Bay University.

mod support;

use harpseal_channel_fake::FakeProfile;
use support::{Chat, boot, sql, text_of};

async fn claim<'a>(c: &mut Chat<'a>, code: &str) -> String {
    let seen = c.text(code).await;
    text_of(&seen[0])
}

async fn ready<'a>(
    h: &'a harpseal_testing::Harness,
    app: &'a harpseal_testing::TestApp,
) -> Chat<'a> {
    let mut c = Chat::new(h, app, "u1");
    c.follow().await;
    c.press("kernel.consent.accept").await;
    c
}

#[tokio::test]
async fn an_invalid_email_on_file_is_refused_like_a_missing_one_and_sends_nothing() {
    let (h, app) = boot(FakeProfile::Full).await;
    sql(
        &h,
        "UPDATE student SET email = 'nok@sealbay' WHERE id = 's1'",
    )
    .await;
    let mut c = ready(&h, &app).await;
    for _ in 0..3 {
        let said = claim(&mut c, "68010001").await;
        assert!(said.contains("อีเมลของรหัสนี้ในทะเบียนไม่ถูกต้อง"), "{said}");
    }
    assert!(h.emails().is_empty(), "no provider call");
    assert_eq!(
        app.count("SELECT count(*) FROM verification_challenge")
            .await,
        0
    );
    assert_eq!(app.count("SELECT count(*) FROM security_event").await, 0);
    // The missing one keeps its own text.
    let said = claim(&mut c, "68010003").await;
    assert!(said.contains("ทะเบียนไม่มีอีเมลของรหัสนี้"), "{said}");
    // Fixed by Staff, the claim goes through.
    sql(
        &h,
        "UPDATE student SET email = 'nok@student.sealbay.example' WHERE id = 's1'",
    )
    .await;
    claim(&mut c, "68010001").await;
    assert_eq!(h.emails().len(), 1);
}

#[tokio::test]
async fn a_shared_email_is_refused_to_both_students_and_recorded_for_staff() {
    let (h, app) = boot(FakeProfile::Full).await;
    sql(
        &h,
        "UPDATE student SET email = 'Share@student.sealbay.example' WHERE id = 's1'",
    )
    .await;
    sql(
        &h,
        "UPDATE student SET email = ' share@STUDENT.sealbay.example ' WHERE id = 's4'",
    )
    .await;
    let mut c = ready(&h, &app).await;
    for code in ["68010001", "68010004"] {
        let said = claim(&mut c, code).await;
        assert!(said.contains("ใช้ร่วมกับนักศึกษาอีกคน"), "{said}");
    }
    assert!(h.emails().is_empty());
    let events = "SELECT count(*) FROM security_event WHERE kind = 'link.shared_email'";
    assert_eq!(app.count(events).await, 2);
    // Staff can find the other Student: the event names the other codes.
    let s1 = "SELECT count(*) FROM security_event WHERE kind = 'link.shared_email' \
              AND student_id = 's1' AND json_extract(details, '$.student_code') = '68010001' \
              AND json_extract(details, '$.shared_with') = '68010004'";
    assert_eq!(app.count(s1).await, 1);
    // The same account claiming the same Student again is not recorded again
    // within the hour, but is refused every time; after the hour it is.
    for _ in 0..3 {
        let said = claim(&mut c, "68010001").await;
        assert!(said.contains("ใช้ร่วมกับนักศึกษาอีกคน"), "{said}");
    }
    assert_eq!(app.count(events).await, 2);
    h.clock().advance_minutes(61);
    claim(&mut c, "68010001").await;
    assert_eq!(app.count(events).await, 3);
    assert!(h.emails().is_empty());
}

#[tokio::test]
async fn a_left_student_with_the_same_email_does_not_block_the_claim() {
    let (h, app) = boot(FakeProfile::Full).await;
    // s2 is left in the roster fixture.
    sql(
        &h,
        "UPDATE student SET email = 'same@student.sealbay.example' WHERE id IN ('s1', 's2')",
    )
    .await;
    let mut c = ready(&h, &app).await;
    claim(&mut c, "68010001").await;
    assert_eq!(h.emails().len(), 1, "the code was sent");
    assert_eq!(app.count("SELECT count(*) FROM security_event").await, 0);
}

#[tokio::test]
async fn a_resend_checks_the_email_again_after_an_import_or_edit_changed_it() {
    let (h, app) = boot(FakeProfile::Full).await;
    let mut c = ready(&h, &app).await;
    claim(&mut c, "68010001").await;
    assert_eq!(h.emails().len(), 1, "the first code went out");
    let pending = "SELECT count(*) FROM verification_challenge WHERE state = 'pending'";
    assert_eq!(app.count(pending).await, 1);

    // Another Student now has the address: the resend is refused.
    sql(
        &h,
        "UPDATE student SET email = (SELECT email FROM student WHERE id = 's1') WHERE id = 's4'",
    )
    .await;
    let seen = c.press("kernel.otp.resend").await;
    assert!(
        text_of(&seen[0]).contains("ใช้ร่วมกับนักศึกษาอีกคน"),
        "{:?}",
        seen
    );
    assert_eq!(
        h.emails().len(),
        1,
        "nothing was sent to the shared mailbox"
    );
    assert_eq!(app.count(pending).await, 0, "the waiting code is cancelled");
    let state = app.query("SELECT flow_state FROM channel_account").await;
    assert_eq!(
        state.get(0, "flow_state").unwrap().as_text(),
        Some("awaiting_student_id")
    );
    assert_eq!(
        app.count("SELECT count(*) FROM security_event WHERE kind = 'link.shared_email'")
            .await,
        1
    );

    // The same for an address that is no longer valid.
    sql(
        &h,
        "UPDATE student SET email = 's68010004@student.sealbay.example' WHERE id = 's4'",
    )
    .await;
    claim(&mut c, "68010001").await;
    assert_eq!(h.emails().len(), 2);
    sql(
        &h,
        "UPDATE student SET email = 'nok@sealbay' WHERE id = 's1'",
    )
    .await;
    let seen = c.press("kernel.otp.resend").await;
    assert!(
        text_of(&seen[0]).contains("อีเมลของรหัสนี้ในทะเบียนไม่ถูกต้อง"),
        "{:?}",
        seen
    );
    assert_eq!(h.emails().len(), 2);
}

#[tokio::test]
async fn the_shared_email_lookup_uses_an_index() {
    let (_h, app) = boot(FakeProfile::Full).await;
    let plan = app
        .query("EXPLAIN QUERY PLAN SELECT student_code FROM student WHERE lower(trim(email)) = 'a@b.cd'")
        .await;
    let detail = plan
        .get(0, "detail")
        .unwrap()
        .as_text()
        .unwrap()
        .to_string();
    assert!(detail.contains("student_email_key_idx"), "{detail}");
}
