//! Mini App follow-ups (#52) at the HTTP seam: the platform script comes
//! from the `MiniApp::web_client` Capability, the entry page's plain
//! fallback outside both chat apps, the pending re-consent line, and the
//! `app_session.expires_at` index.

mod announcements_support;
mod app_support;

use announcements_support::*;
use app_support::*;
use harpseal::{Deployment, EmailOtp, WebClient};
use harpseal_testing::Harness;

const PENDING: &str = "มีประกาศความเป็นส่วนตัวฉบับใหม่ กรุณายอมรับในแชท";

#[tokio::test]
async fn the_script_follows_the_web_client_not_the_kind_or_url() {
    let h = Harness::new();
    // A Telegram Web App under another kind, and a LIFF app whose launch
    // URL is the Fake's (no `https://liff.line.me/` anywhere).
    let campus = h
        .fake_channel()
        .with_kind("campus")
        .with_web_client(WebClient::Telegram);
    let liff = h
        .fake_channel()
        .with_kind("lineoa")
        .with_web_client(WebClient::Liff {
            id: "2000000000-Campus0".into(),
        });
    let d = Deployment::builder()
        .config(Harness::config())
        .runtime(h.services())
        .channel(h.fake_channel())
        .channel(campus)
        .channel(liff)
        .verification(EmailOtp::new(Harness::config().otp))
        .build()
        .unwrap();
    let app = h.boot(d).await;
    let page = app.get("/app").await.body;
    assert!(page.contains("src=\"https://telegram.org/js/telegram-web-app.js\""));
    assert!(page.contains("data-tg=\"campus\""), "{page}");
    assert!(page.contains("src=\"https://static.line-scdn.net/liff/edge/2/sdk.js\""));
    assert!(page.contains("data-liff=\"2000000000-Campus0\""));
    assert!(page.contains("data-line=\"lineoa\""));
}

#[tokio::test]
async fn a_channel_without_a_web_client_gets_no_platform_script() {
    let w = boot().await;
    let page = w.app.get("/app").await.body;
    assert!(page.contains("id=\"xchg\""));
    assert!(!page.contains("telegram-web-app.js"));
    assert!(!page.contains("liff/edge"));
    assert!(!page.contains("data-tg=") && !page.contains("data-liff="));
}

#[tokio::test]
async fn outside_both_chat_apps_the_entry_page_links_the_notice_and_staff_sign_in() {
    let w = boot().await;
    // The bare domain: `/` forwards to `/app`, the entry page.
    let root = w.app.get("/").await;
    assert_eq!(root.header("location"), Some("/app"));
    let page = w.app.get("/app").await.body;
    // Without script: the plain links in <noscript>, and no "opening" text
    // left showing (the script unhides it).
    let noscript = &page[page.find("<noscript>").expect("a noscript part")..];
    let noscript = &noscript[..noscript.find("</noscript>").unwrap()];
    for link in ["href=\"/notice\"", "href=\"/admin/login\""] {
        assert!(noscript.contains(link), "{link}: {noscript}");
        assert!(page.matches(link).count() >= 2, "also in #xchg-out");
    }
    assert!(noscript.contains("เปิดจากปุ่มในแชท LINE หรือ Telegram"));
    assert!(page.contains("id=\"xchg-wait\" hidden"), "{page}");
    assert!(page.contains("id=\"xchg-out\" hidden"));
    // The script decides between the exchange, "open from the chat" and
    // the plain page; a framed page says so to the exchange.
    assert!(page.contains("show('xchg-out')"));
    assert!(page.contains("name=\"framed\""));
    // No LIFF here: no LINE Login button.
    assert!(!page.contains("id=\"xchg-line\""));
}

#[tokio::test]
async fn a_newer_notice_version_shows_the_reconsent_line() {
    let w = boot().await;
    roster(&w).await;
    let b = open(&w, "fake", "s1").await;
    let page = b.get("/app").await.body;
    assert!(page.contains("ยอมรับประกาศความเป็นส่วนตัวฉบับ 1 แล้ว"));
    assert!(!page.contains(PENDING));

    // Staff publish version 2026-11: the page keeps the accepted version
    // and says a re-consent waits in the chat.
    run(
        &w.h,
        "INSERT INTO setting (key, locale, value, updated_at) \
         VALUES ('kernel.consent.version', '', '2026-11', 1)",
    )
    .await;
    let page = b.get("/app").await.body;
    assert!(page.contains("ยอมรับประกาศความเป็นส่วนตัวฉบับ 1 แล้ว"));
    assert!(page.contains(PENDING), "{page}");

    // Accepting the new version (in the chat) clears it, whatever order
    // the rows were written in.
    run(
        &w.h,
        "INSERT INTO consent (id, student_id, channel_account_id, notice_version, locale, \
         accepted_at) VALUES ('c-new', 's1', 'a-fake-s1', '2026-11', 'th', 2)",
    )
    .await;
    let page = b.get("/app").await.body;
    assert!(page.contains("ยอมรับประกาศความเป็นส่วนตัวฉบับ 2026-11 แล้ว"));
    assert!(!page.contains(PENDING));
}

#[tokio::test]
async fn a_student_with_no_consent_at_all_sees_no_reconsent_line() {
    let w = boot().await;
    roster(&w).await;
    // Seeded data: a Link and no Consent (the chat flow leaves it alone).
    student(&w.h, "s5", "th", "active", "{}").await;
    run(
        &w.h,
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
         updated_at) VALUES ('a-fake-s5', 'fake', 'fake-s5', 1, 'linked', 1)",
    )
    .await;
    run(
        &w.h,
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
         linked_at, updated_at) VALUES ('l-fake-s5', 's5', 'a-fake-s5', 'fake', 'active', \
         'manual', 1, 1)",
    )
    .await;
    run(
        &w.h,
        "INSERT INTO setting (key, locale, value, updated_at) \
         VALUES ('kernel.consent.version', '', '2026-11', 1)",
    )
    .await;
    let page = open(&w, "fake", "s5").await.get("/app").await.body;
    assert!(page.contains("ยังไม่ได้ยอมรับประกาศความเป็นส่วนตัว"));
    assert!(!page.contains(PENDING));
}

#[tokio::test]
async fn the_prune_job_has_an_expires_at_index() {
    let w = boot().await;
    let n = w
        .app
        .count(
            "SELECT count(*) FROM sqlite_master WHERE type = 'index' \
             AND name = 'app_session_expires_idx' AND tbl_name = 'app_session'",
        )
        .await;
    assert_eq!(n, 1);
}
