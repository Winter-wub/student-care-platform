//! The edit notice's scope (#77 review): what the box says it costs, only
//! a changed text, only the Students whose text changed, never for an
//! expired announcement (nor a resend of one).

mod announcements_support;

use announcements_support::notice::*;
use announcements_support::*;
use harpseal_testing::urlencode;

#[tokio::test]
async fn the_box_shows_line_recipients_and_the_quota_and_warns_when_over() {
    // 2 Pushes for the send leave 1 of 3; the notice would need 2.
    let src = format!("{CONFIG}\n[quotas.fake]\nmetered_per_month = 3\n");
    let w = boot_config(&src, 500).await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b).await;
    let page = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    let cost = &page[page.find("id=\"notify-cost\"").expect("cost shown")..];
    assert!(cost.contains("แจ้งได้สูงสุด 2 คน"), "{cost}");
    assert!(cost.contains("ใช้ไปแล้ว 2/3 เดือนนี้"), "{cost}");
    assert!(
        cost.contains("เกินโควตาที่เหลือของเดือนนี้ (เหลือ 1 ครั้ง)"),
        "{cost}"
    );
    // Allowed anyway: the Quota warns, it does not block.
    edit(&w, &b, &id, "ใหม่", true).await;
    drain(&w).await;
    assert_eq!(notices(&w, "s1").len() + notices(&w, "s3").len(), 2);

    // With room left, no warning.
    let src = format!("{CONFIG}\n[quotas.fake]\nmetered_per_month = 300\n");
    let w = boot_config(&src, 500).await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b).await;
    let page = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(page.contains("ใช้ไปแล้ว 2/300 เดือนนี้"));
    assert!(!page.contains("เกินโควตาที่เหลือ"));
}

#[tokio::test]
async fn an_edit_without_a_changed_text_refuses_the_notice() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b).await;
    // Only the expiry changes, box ticked: refused, nothing saved or sent.
    let r = post_edit(
        &w,
        &b,
        &id,
        &form("เดิม", "ข้อความเดิม", "&expires=2026-12-31&notify=1"),
    )
    .await;
    assert_eq!(r.status, 422);
    assert!(r.body.contains("ข้อความไม่ได้เปลี่ยน จึงไม่มีอะไรต้องแจ้ง"));
    drain(&w).await;
    assert!(notices(&w, "s1").is_empty());
    assert_eq!(
        texts(
            &w,
            "SELECT coalesce(notice_report, 'none') FROM announcement"
        )
        .await,
        ["none"]
    );
}

#[tokio::test]
async fn only_students_whose_text_changed_are_notified() {
    let w = boot().await;
    roster(&w).await;
    // s5 reads English, on the LINE-like Channel.
    student(
        &w.h,
        "s5",
        "en",
        "active",
        r#"{"programme":"MSC","year":2}"#,
    )
    .await;
    link(&w.h, "s5", "fake", "active").await;
    let b = signed_in(&w, "Officer").await;
    let en = |t: &str| {
        format!(
            "&title_en={}&body_en={}",
            urlencode(t),
            urlencode("Details")
        )
    };
    let id = create(
        &b,
        &form("เดิม", "ข้อความเดิม", &format!("{}&do=confirm", en("Old"))),
    )
    .await;
    drain(&w).await;
    w.line.take_sent();
    // Only the English text changes: s5 alone.
    let r = post_edit(
        &w,
        &b,
        &id,
        &form("เดิม", "ข้อความเดิม", &format!("{}&notify=1", en("New"))),
    )
    .await;
    assert_eq!(r.status, 303, "{}", r.body);
    drain(&w).await;
    assert_eq!(notices(&w, "s5").len(), 1);
    assert!(notices(&w, "s1").is_empty() && notices(&w, "s3").is_empty());
    assert_eq!(
        texts(&w, "SELECT notice_report ->> 'total' FROM announcement").await,
        ["1"]
    );
    // Now the Thai: the Thai readers (s5's English is unchanged).
    let r = post_edit(
        &w,
        &b,
        &id,
        &form("ใหม่", "ข้อความเดิม", &format!("{}&notify=1", en("New"))),
    )
    .await;
    assert_eq!(r.status, 303, "{}", r.body);
    drain(&w).await;
    assert_eq!((notices(&w, "s1").len(), notices(&w, "s3").len()), (1, 1));
    assert_eq!(notices(&w, "s5").len(), 1);
}

#[tokio::test]
async fn an_expired_announcement_gets_no_notice_and_no_resend() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b).await;
    edit(&w, &b, &id, "ใหม่", true).await;
    // It expires before the notice runs: the run stops.
    let now = w.h.clock().now_ms();
    run(
        &w.h,
        &format!(
            "UPDATE announcement SET expires_at = {} WHERE id = '{id}'",
            now - 1
        ),
    )
    .await;
    drain(&w).await;
    assert!(notices(&w, "s1").is_empty() && notices(&w, "s3").is_empty());
    // "ส่งซ้ำ" is gone, and a posted resend is refused.
    let detail = format!("/admin/announcements/{id}");
    let page = b.get(&detail).await.body;
    assert!(!page.contains("/resend"), "{page}");
    let v = version(&w, &id).await;
    let r = resend_confirmed(&b, &id, v).await;
    assert_eq!(r.status, 303);
    let after = b.get(&detail).await.body;
    assert!(after.contains("ข่าวนี้หมดอายุแล้ว ขยายวันหมดอายุก่อนจึงส่งซ้ำได้"));
    assert_eq!(
        texts(&w, "SELECT round || status FROM announcement").await,
        ["0sent"]
    );
}

#[tokio::test]
async fn an_expired_announcement_offers_no_box_and_a_ticked_one_is_refused() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b).await;
    let now = w.h.clock().now_ms();
    run(
        &w.h,
        &format!(
            "UPDATE announcement SET expires_at = {} WHERE id = '{id}'",
            now - 1
        ),
    )
    .await;
    let page = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(!page.contains("name=\"notify\""), "no box once expired");
    // Posted anyway (with a new expiry and a new text): an error, not a
    // silent drop.
    let r = post_edit(
        &w,
        &b,
        &id,
        &form("ใหม่", "ข้อความใหม่", "&expires=2026-12-31&notify=1"),
    )
    .await;
    assert_eq!(r.status, 422);
    assert!(
        r.body
            .contains("ข่าวนี้หมดอายุแล้ว (หรือจะหมดอายุเมื่อบันทึก) จึงแจ้งไม่ได้")
    );
    drain(&w).await;
    assert!(notices(&w, "s1").is_empty() && notices(&w, "s3").is_empty());
    assert_eq!(
        texts(
            &w,
            "SELECT title FROM announcement_text WHERE locale = 'th'"
        )
        .await,
        ["เดิม"],
        "nothing saved"
    );
}

#[tokio::test]
async fn an_english_reader_without_english_text_is_notified_of_a_thai_change() {
    let w = boot().await;
    roster(&w).await;
    // s5 reads English; the announcement has Thai only (s5 falls back).
    student(
        &w.h,
        "s5",
        "en",
        "active",
        r#"{"programme":"MSC","year":2}"#,
    )
    .await;
    link(&w.h, "s5", "fake", "active").await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b).await;
    edit(&w, &b, &id, "ใหม่", true).await;
    drain(&w).await;
    assert_eq!(notices(&w, "s5"), ["ใหม่"]);
    assert_eq!(
        texts(&w, "SELECT notice_report ->> 'total' FROM announcement").await,
        ["3"]
    );
}
