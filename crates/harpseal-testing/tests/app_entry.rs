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
    // Without script: the plain links in <noscript>, and the "opening"
    // text hidden by a <noscript> style.
    let links = page
        .find("<noscript><div")
        .expect("a noscript part with the links");
    let noscript = &page[links..];
    let noscript = &noscript[..noscript.find("</noscript>").unwrap()];
    for link in ["href=\"/notice\"", "href=\"/admin/login\""] {
        assert!(noscript.contains(link), "{link}: {noscript}");
        assert!(page.matches(link).count() >= 2, "also in #xchg-out");
    }
    assert!(noscript.contains("เปิดจากปุ่มในแชท LINE หรือ Telegram"));
    assert!(page.contains("<noscript><style>#xchg-wait{display:none}</style></noscript>"));
    assert!(
        !page.contains("id=\"xchg-wait\" hidden"),
        "shown while the SDKs load"
    );
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

    // A Consent row for the new version (as the chat writes on accept)
    // clears it.
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

#[tokio::test]
async fn app_pages_may_be_framed_only_by_themselves_and_telegram_web() {
    let w = boot().await;
    roster(&w).await;
    seeded(&w, "n1", "s1", "fake", 1_000, true, None).await;
    let csp = "frame-ancestors 'self' https://web.telegram.org";
    // The entry page, the Student's page, a detail page and the ack.
    assert_eq!(
        w.app.get("/app").await.header("content-security-policy"),
        Some(csp)
    );
    let b = open(&w, "fake", "s1").await;
    for path in ["/app", "/app/announcements", "/app/announcements/n1"] {
        let r = b.get(path).await;
        assert_eq!(r.status, 200, "{path}");
        assert_eq!(r.header("content-security-policy"), Some(csp), "{path}");
    }
    let ack = b.post_raw("/app/announcements/n1/ack", "").await;
    assert_eq!(ack.header("content-security-policy"), Some(csp));
    // The back office stays unframeable, and other pages are untouched.
    assert!(
        w.app
            .get("/notice")
            .await
            .header("content-security-policy")
            .is_none()
    );
}

#[tokio::test]
async fn a_cross_site_ack_is_refused_even_with_the_session_and_token() {
    let w = boot().await;
    roster(&w).await;
    seeded(&w, "n1", "s1", TG, 1_000, true, None).await;
    // A Telegram Web session (its cookie is sent in the iframe).
    let b = open(&w, TG, "s1").await;
    let page = b.get("/app/announcements/n1").await.body;
    let csrf = csrf_of(&page);
    let post = |form: String| {
        http::Request::post("/app/announcements/n1/ack")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("sec-fetch-site", "cross-site")
            .body(axum::body::Body::from(form))
            .unwrap()
    };
    assert_eq!(b.send(post(String::new())).await.status, 403);
    assert_eq!(b.send(post(format!("_csrf={csrf}"))).await.status, 403);
    let acked = w
        .app
        .count("SELECT count(*) FROM announcement_delivery WHERE acked_at IS NOT NULL")
        .await;
    assert_eq!(acked, 0);
    // The same post from our own page goes through.
    let ok = http::Request::post("/app/announcements/n1/ack")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin")
        .body(axum::body::Body::from(format!("_csrf={csrf}")))
        .unwrap();
    assert_eq!(b.send(ok).await.status, 303);
}
