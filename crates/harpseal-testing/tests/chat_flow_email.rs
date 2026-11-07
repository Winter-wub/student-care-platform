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
async fn a_shared_email_is_refused_to_both_students_and_recorded() {
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
    claim(&mut c, "68010001").await;
    assert_eq!(app.count(events).await, 3, "every attempt is recorded");
    let s1 = "SELECT count(*) FROM security_event WHERE kind = 'link.shared_email' \
              AND student_id = 's1' AND json_extract(details, '$.student_code') = '68010001'";
    assert_eq!(app.count(s1).await, 2);
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
