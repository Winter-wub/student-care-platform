//! A send that reaches nobody (#135): the review step disables "ยืนยันส่ง"
//! and says why, and a confirm posted anyway is refused, nothing queued.

mod announcements_support;

use announcements_support::*;

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
