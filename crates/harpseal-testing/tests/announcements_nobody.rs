//! A send that reaches nobody (#135): when it would go now, the review
//! step disables "ยืนยันส่ง" and says why, and a confirm posted anyway is
//! refused, nothing queued. A schedule still ahead resolves its recipients
//! when it goes out, so it stays confirmable, with a caution note.

mod announcements_support;

use announcements_support::*;

/// 2026-11-05 07:00 in +07:00 is the harness START; this is 09:30.
const LATER: &str = "&when=later&send_at=2026-11-05T09%3A30";
const NOTE_LATER: &str = "ยังไม่มีผู้รับตอนนี้ ระบบจะนับผู้รับใหม่ตอนถึงเวลาส่ง";
/// The deliver Jobs queued.
const QUEUED: &str =
    "SELECT count(*) FROM job WHERE kind = 'announcements.deliver' AND state = 'queued'";
const BLOCKED: &str =
    r#"<button type="submit" disabled aria-describedby="review-blocked">ยืนยันส่ง</button>"#;

fn cookie_flash(b: &harpseal_testing::Browser<'_>) -> String {
    b.cookie("hs_flash").unwrap_or_default()
}

/// Posts the review step's confirm for `id` as a browser that skipped
/// the disabled button would, and checks it was refused.
async fn refused(w: &World, b: &harpseal_testing::Browser<'_>, id: &str) {
    let v = version(w, id).await;
    let r = b
        .post(
            &format!("/admin/announcements/{id}/confirm"),
            &format!("version={v}&reviewed=1&reach=0"),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    let review = format!("/admin/announcements/{id}/review");
    assert_eq!(r.header("location"), Some(review.as_str()));
    assert_eq!(cookie_flash(b), "err:announcements.review.nobody_refused");
    assert_eq!(texts(w, "SELECT status FROM announcement").await, ["draft"]);
    assert_eq!(
        count(w, "SELECT count(*) FROM job").await,
        0,
        "nothing queued"
    );
}

#[tokio::test]
async fn nobody_linked_disables_the_send_with_the_reason() {
    let w = boot().await;
    student(
        &w.h,
        "s1",
        "th",
        "active",
        r#"{"programme":"MSC","year":2}"#,
    )
    .await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ข่าว", "เนื้อหา", "")).await;
    let page = b.get(&format!("/admin/announcements/{id}/review")).await;
    assert_eq!(page.status, 200);
    let body = &page.body;
    assert!(body.contains(BLOCKED), "{body}");
    assert!(
        body.contains(
            r#"<p class="blocked-reason" id="review-blocked">ยังไม่มีนักศึกษาที่ผูกบัญชีในกลุ่มผู้รับนี้"#
        ),
        "{body}"
    );
    assert!(!body.contains("/confirm\""), "no confirm form: {body}");
    assert!(!body.contains("ส่งถึงทุกคน"), "no danger button: {body}");
    assert!(body.contains("กลับไปแก้ไข"), "{body}");
    refused(&w, &b, &id).await;
}

#[tokio::test]
async fn only_unreachable_links_disable_the_send_too() {
    let w = boot().await;
    student(
        &w.h,
        "s1",
        "th",
        "active",
        r#"{"programme":"MSC","year":2}"#,
    )
    .await;
    link(&w.h, "s1", "fake", "unreachable").await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ข่าว", "เนื้อหา", "&unreachable=1")).await;
    let body = b
        .get(&format!("/admin/announcements/{id}/review"))
        .await
        .body;
    assert!(body.contains(BLOCKED), "{body}");
    assert!(body.contains("ติดต่อไม่ได้ทุกบัญชี"), "{body}");
    refused(&w, &b, &id).await;
}

#[tokio::test]
async fn one_reachable_student_enables_the_send_again() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(
        &b,
        &form("ข่าว", "เนื้อหา", "&scope=students&students=code-s3"),
    )
    .await;
    let body = b
        .get(&format!("/admin/announcements/{id}/review"))
        .await
        .body;
    assert!(!body.contains(r#"disabled aria-describedby="#), "{body}");
    assert!(!body.contains("blocked-reason"), "{body}");
    let r = confirm_reviewed(&b, &id).await;
    assert_eq!(r.status, 303);
    assert_eq!(cookie_flash(&b), "ok:announcements.flash.sending");
}

#[tokio::test]
async fn a_schedule_ahead_with_nobody_yet_is_confirmable_with_a_note() {
    let w = boot().await;
    student(
        &w.h,
        "s1",
        "th",
        "active",
        r#"{"programme":"MSC","year":2}"#,
    )
    .await;
    let b = signed_in(&w, "Officer").await;
    let id = create(
        &b,
        &form(
            "นัดหมาย",
            "พบอาจารย์",
            &format!("&scope=students&students=code-s1{LATER}"),
        ),
    )
    .await;
    let body = b
        .get(&format!("/admin/announcements/{id}/review"))
        .await
        .body;
    assert!(!body.contains(BLOCKED), "{body}");
    assert!(body.contains(NOTE_LATER), "{body}");
    let r = confirm_reviewed(&b, &id).await;
    assert_eq!(r.status, 303);
    assert_eq!(cookie_flash(&b), "ok:announcements.flash.scheduled");
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["scheduled"]
    );
    assert_eq!(count(&w, QUEUED).await, 1, "the schedule's Job");
}

#[tokio::test]
async fn a_schedule_already_past_with_nobody_is_refused_as_now() {
    let w = boot().await;
    student(
        &w.h,
        "s1",
        "th",
        "active",
        r#"{"programme":"MSC","year":2}"#,
    )
    .await;
    let b = signed_in(&w, "Officer").await;
    let id = create(
        &b,
        &form(
            "นัดหมาย",
            "พบอาจารย์",
            &format!("&scope=students&students=code-s1{LATER}"),
        ),
    )
    .await;
    // The draft waits past its send_at: confirming it now sends at once.
    w.h.clock().advance_minutes(160);
    let body = b
        .get(&format!("/admin/announcements/{id}/review"))
        .await
        .body;
    assert!(body.contains(BLOCKED), "{body}");
    assert!(!body.contains(NOTE_LATER), "{body}");
    refused(&w, &b, &id).await;
}

#[tokio::test]
async fn a_held_schedule_edited_to_nobody_is_still_confirmable() {
    let w = boot().await;
    roster(&w).await;
    student(
        &w.h,
        "s5",
        "th",
        "active",
        r#"{"programme":"MSC","year":1}"#,
    )
    .await;
    let b = signed_in(&w, "Officer").await;
    let extra = format!("&scope=groups&attr.programme=MSC{LATER}&do=confirm");
    let id = create(&b, &form("นัดหมาย", "พบอาจารย์", &extra)).await;
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["scheduled"]
    );
    // The Audience changes to s5 alone (no Link): held for review.
    let path = format!("/admin/announcements/{id}/edit");
    b.get(&path).await;
    let v = version(&w, &id).await;
    let edited = format!("&scope=students&students=code-s5{LATER}&version={v}");
    let r = b.post(&path, &form("นัดหมาย", "พบอาจารย์", &edited)).await;
    let held = format!("/admin/announcements/{id}/review?held=1");
    assert_eq!(r.header("location"), Some(held.as_str()), "{}", r.body);
    let body = b.get(&held).await.body;
    assert!(!body.contains(BLOCKED), "{body}");
    assert!(body.contains(NOTE_LATER), "{body}");
    confirm_reviewed(&b, &id).await;
    assert_eq!(cookie_flash(&b), "ok:announcements.flash.scheduled");
    assert_eq!(count(&w, QUEUED).await, 1);
}
