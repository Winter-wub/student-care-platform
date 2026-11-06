//! Mini App (#28) pages: the Student's announcements, one announcement,
//! the one-way acknowledge, and `/notice`.

mod announcements_support;
mod app_support;

use announcements_support::*;
use app_support::*;

/// Where `needle` first appears in `hay` (panics when absent).
fn at(hay: &str, needle: &str) -> usize {
    hay.find(needle)
        .unwrap_or_else(|| panic!("{needle} missing"))
}

#[tokio::test]
async fn the_list_puts_unacknowledged_first_and_hides_retracted_and_expired() {
    let w = boot().await;
    roster(&w).await;
    let now = w.h.clock().now_ms();
    seeded(&w, "old-wait", "s1", "fake", now - 5_000, true, None).await;
    seeded(&w, "new-info", "s1", "fake", now - 1_000, false, None).await;
    seeded(&w, "mid-acked", "s1", "fake", now - 3_000, true, None).await;
    seeded(&w, "gone", "s1", "fake", now - 500, false, None).await;
    seeded(&w, "stale", "s1", "fake", now - 400, false, Some(now - 1)).await;
    seeded(&w, "other", "s3", "fake", now - 300, false, None).await;
    run(
        &w.h,
        "UPDATE announcement SET status = 'retracted' WHERE id = 'gone'",
    )
    .await;
    run(
        &w.h,
        "UPDATE announcement_delivery SET acked_at = 1 WHERE announcement_id = 'mid-acked'",
    )
    .await;

    let b = open(&w, "fake", "s1").await;
    let page = b.get("/app").await.body;
    assert!(page.contains("ข่าวของฉัน"));
    // Waiting first, then newest first.
    assert!(at(&page, "ข่าว old-wait") < at(&page, "ข่าว new-info"));
    assert!(at(&page, "ข่าว new-info") < at(&page, "ข่าว mid-acked"));
    for hidden in ["ข่าว gone", "ข่าว stale", "ข่าว other"] {
        assert!(!page.contains(hidden), "{hidden}");
    }
    // Title, two clamped lines and the local date; expands in place.
    assert!(page.contains("<details>"));
    assert!(page.contains("class=\"clamp\""));
    assert!(page.contains("2026-11-05 06:59"), "{page}");
    assert!(page.contains("รอการรับทราบ"));
    // One button (old-wait), and the acknowledged line for mid-acked.
    assert_eq!(page.matches("class=\"app-ack\"").count(), 1);
    assert_eq!(page.matches("class=\"app-acked\"").count(), 1);
}

#[tokio::test]
async fn a_student_with_no_announcements_sees_the_empty_text() {
    let w = boot().await;
    roster(&w).await;
    let b = open(&w, TG, "s4").await;
    let page = b.get("/app").await.body;
    assert!(page.contains("ยังไม่มีข่าวสำหรับคุณ") || page.contains("No announcements"));
    // s4's Locale is English.
    assert!(page.contains("My announcements"), "{page}");
    assert!(page.contains("Linked through tg"));
}

#[tokio::test]
async fn the_detail_shows_thai_when_the_students_locale_has_no_text() {
    let w = boot().await;
    roster(&w).await;
    let now = w.h.clock().now_ms();
    seeded(&w, "thai-only", "s4", TG, now, false, None).await;
    seeded(&w, "both", "s4", TG, now, false, None).await;
    run(
        &w.h,
        "INSERT INTO announcement_text (announcement_id, locale, title, body) \
         VALUES ('both', 'en', 'News both', 'Details both')",
    )
    .await;
    let b = open(&w, TG, "s4").await;
    let thai = b.get("/app/announcements/thai-only").await;
    assert_eq!(thai.status, 200);
    assert!(thai.body.contains("ข่าว thai-only"));
    assert!(thai.body.contains("รายละเอียด thai-only"));
    assert!(thai.body.contains("Back to my page"));
    let en = b.get("/app/announcements/both").await.body;
    assert!(en.contains("News both") && en.contains("Details both"));
    assert!(!en.contains("ข่าว both"));
}

#[tokio::test]
async fn acknowledging_on_the_page_marks_every_row_and_chat_then_says_already() {
    let w = boot().await;
    roster(&w).await;
    let officer = signed_in(&w, "Officer").await;
    let id = create(
        &officer,
        &form("สอบกลางภาค", "ตารางสอบออกแล้ว", "&require_ack=1&do=confirm"),
    )
    .await;
    drain(&w).await;
    // s1 got it on both Channels.
    let rows = format!(
        "SELECT count(*) FROM announcement_delivery WHERE announcement_id = '{id}' \
         AND student_id = 's1' AND acked_at IS NOT NULL"
    );
    assert_eq!(count(&w, &rows).await, 0);

    let b = open(&w, "fake", "s1").await;
    let detail = b.get(&format!("/app/announcements/{id}")).await;
    assert_eq!(detail.status, 200);
    assert!(detail.body.contains("ตารางสอบออกแล้ว"));
    let ack = format!("/app/announcements/{id}/ack");
    assert!(detail.body.contains(&format!("hx-post=\"{ack}\"")));
    let csrf = csrf_of(&detail.body);
    let r = hx_post(&b, &ack, &format!("_csrf={csrf}")).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body.contains("รับทราบแล้ว"));
    assert!(!r.body.contains("<html"));
    assert_eq!(count(&w, &rows).await, 2);

    // The chat Action on the other Channel now answers "already".
    let again = press(&w, TG, "s1", &format!("announcements.ack.{id}")).await;
    assert_eq!(text(&again[0]), "รับทราบแล้ว");
    // The page shows the line instead of the button; a plain form post
    // (no script) comes back to the detail page.
    let detail = b.get(&format!("/app/announcements/{id}")).await.body;
    assert!(!detail.contains("class=\"app-ack\""));
    assert!(detail.contains("class=\"app-acked\""));
    let plain = b.post_raw(&ack, &format!("_csrf={csrf}")).await;
    assert_eq!(plain.status, 303);
    assert_eq!(
        plain.header("location"),
        Some(format!("/app/announcements/{id}").as_str())
    );
}

#[tokio::test]
async fn an_announcement_the_student_was_not_sent_is_404_for_page_and_ack() {
    let w = boot().await;
    roster(&w).await;
    let now = w.h.clock().now_ms();
    seeded(&w, "for-s1", "s1", "fake", now, true, None).await;
    seeded(&w, "for-s3", "s3", "fake", now, true, None).await;
    let b = open(&w, "fake", "s3").await;
    let csrf = csrf_of(&b.get("/app").await.body);
    for id in ["for-s1", "does-not-exist"] {
        let page = b.get(&format!("/app/announcements/{id}")).await;
        assert_eq!(page.status, 404, "{id}");
        assert!(!page.body.contains("ข่าว for-s1"));
        let r = hx_post(
            &b,
            &format!("/app/announcements/{id}/ack"),
            &format!("_csrf={csrf}"),
        )
        .await;
        assert_eq!(r.status, 404, "{id}");
    }
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM announcement_delivery WHERE acked_at IS NOT NULL"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn a_retracted_announcement_shows_the_text_and_takes_no_ack() {
    let w = boot().await;
    roster(&w).await;
    let now = w.h.clock().now_ms();
    seeded(&w, "pulled", "s1", "fake", now, true, None).await;
    let b = open(&w, "fake", "s1").await;
    let csrf = csrf_of(&b.get("/app").await.body);
    run(
        &w.h,
        "UPDATE announcement SET status = 'retracted' WHERE id = 'pulled'",
    )
    .await;
    let page = b.get("/app/announcements/pulled").await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("ข่าวนี้ถูกถอนแล้ว"));
    assert!(!page.body.contains("รายละเอียด pulled"));
    assert!(!page.body.contains("class=\"app-ack\""));
    let r = hx_post(
        &b,
        "/app/announcements/pulled/ack",
        &format!("_csrf={csrf}"),
    )
    .await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("ข่าวนี้ถูกถอนแล้ว"));
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM announcement_delivery WHERE acked_at IS NOT NULL"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn an_ack_without_the_csrf_token_or_session_is_refused() {
    let w = boot().await;
    roster(&w).await;
    let now = w.h.clock().now_ms();
    seeded(&w, "ask", "s1", "fake", now, true, None).await;
    let b = open(&w, "fake", "s1").await;
    let path = "/app/announcements/ask/ack";
    assert_eq!(b.post_raw(path, "").await.status, 403);
    assert_eq!(b.post_raw(path, "_csrf=00ff").await.status, 403);
    // Without a session: HTMX is sent to the detail page (the entry page
    // runs there); a plain post gets the entry page itself.
    let stranger = w.app.browser();
    let hx = hx_post(&stranger, path, "_csrf=00ff").await;
    assert_eq!(hx.header("hx-redirect"), Some("/app/announcements/ask"));
    let plain = stranger.post_raw(path, "").await;
    assert!(
        plain
            .body
            .contains("name=\"next\" value=\"/app/announcements/ask\"")
    );
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM announcement_delivery WHERE acked_at IS NOT NULL"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn the_notice_is_public_plain_or_in_the_app_shell() {
    let w = boot().await;
    let plain = w.app.get("/notice").await;
    assert_eq!(plain.status, 200);
    assert!(plain.body.contains("ประกาศความเป็นส่วนตัว"));
    assert!(plain.body.contains("ฉบับที่ 1"), "{}", plain.body);
    assert!(!plain.body.contains("href=\"/app\""));
    let app = w.app.get("/notice?app=1").await;
    assert_eq!(app.status, 200);
    assert!(app.body.contains("href=\"/app\""));
    assert!(app.body.contains("href=\"/notice?app=1\""));
    // A Staff-bumped version shows at once.
    run(
        &w.h,
        "INSERT INTO setting (key, locale, value, updated_at) \
         VALUES ('kernel.consent.version', '', '2026-11', 1)",
    )
    .await;
    assert!(w.app.get("/notice").await.body.contains("ฉบับที่ 2026-11"));
}
