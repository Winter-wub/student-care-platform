//! Mini App (#28): exchanging the platform token for an App Session, and
//! what the session sees (Fake Channels: the token is the external id).

mod announcements_support;
mod app_support;

use announcements_support::*;
use app_support::*;
use harpseal::{Deployment, EmailOtp, Timestamp};
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::Harness;

#[tokio::test]
async fn an_exchange_for_a_linked_account_sets_the_cookie_and_opens_the_page() {
    let w = boot().await;
    roster(&w).await;
    let b = w.app.browser();
    let r = b.open_mini_app("fake", "fake-s1", "/app").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(r.header("location"), Some("/app"));
    let cookie = r.header("set-cookie").unwrap();
    for part in [
        "app_session=",
        "Path=/app",
        "HttpOnly",
        "SameSite=Lax",
        "Max-Age=3600",
        "Secure",
    ] {
        assert!(cookie.contains(part), "{cookie}");
    }
    // The row holds the token's hash, never the token.
    let token = b.cookie("app_session").unwrap();
    let ids = texts(&w, "SELECT id FROM app_session").await;
    assert_eq!(ids.len(), 1);
    assert_ne!(ids[0], token);
    assert_eq!(
        texts(&w, "SELECT channel_account_id FROM app_session").await,
        ["a-fake-s1"]
    );

    let page = b.get("/app").await;
    assert_eq!(page.status, 200);
    assert_eq!(page.header("cache-control"), Some("no-store"));
    for shown in [
        "Name s1",
        "รหัสนักศึกษา code-s1",
        "ยอมรับประกาศความเป็นส่วนตัวฉบับ 1 แล้ว",
    ] {
        assert!(page.body.contains(shown), "{shown}: {}", page.body);
    }
    // Server-rendered: no exchange script once the session exists.
    assert!(!page.body.contains("id=\"xchg\""));
    // The list path is the same page.
    assert_eq!(b.get("/app/announcements").await.status, 200);
}

#[tokio::test]
async fn a_refused_token_is_a_403_with_a_security_event_and_no_session() {
    let w = boot().await;
    roster(&w).await;
    let b = w.app.browser();
    let r = b.open_mini_app("fake", "", "/app").await;
    assert_eq!(r.status, 403);
    assert!(r.body.contains("ยืนยันบัญชีแชทของคุณไม่สำเร็จ"), "{}", r.body);
    assert!(r.body.contains("ลองอีกครั้ง"));
    assert!(r.header("set-cookie").is_none());
    assert_eq!(
        texts(
            &w,
            "SELECT json_extract(details, '$.channel') FROM security_event \
             WHERE kind = 'app.session_refused'"
        )
        .await,
        ["fake"]
    );
    assert_eq!(count(&w, "SELECT count(*) FROM app_session").await, 0);
}

#[tokio::test]
async fn unknown_channels_and_channels_without_a_mini_app_are_404() {
    let w = boot().await;
    let b = w.app.browser();
    assert_eq!(b.open_mini_app("nope", "x", "/app").await.status, 404);

    let h = Harness::new();
    let plain = FakeChannel::new("plain-secret")
        .unwrap()
        .with_kind("plain")
        .without_mini_app();
    let d = Deployment::builder()
        .config(Harness::config())
        .runtime(h.services())
        .channel(plain)
        .verification(EmailOtp::new(Harness::config().otp))
        .build()
        .unwrap();
    let app = h.boot(d).await;
    let r = app
        .browser()
        .open_mini_app("plain", "someone", "/app")
        .await;
    assert_eq!(r.status, 404);
}

#[tokio::test]
async fn unlinked_unreachable_and_unknown_accounts_see_the_go_back_text() {
    let w = boot().await;
    roster(&w).await;
    // s3's Link is unreachable (blocked); s5's account never linked.
    run(
        &w.h,
        "UPDATE link SET status = 'unreachable' WHERE id = 'l-fake-s3'",
    )
    .await;
    run(
        &w.h,
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
         updated_at) VALUES ('a-fake-s5', 'fake', 'fake-s5', 1, 'awaiting_student_id', 1)",
    )
    .await;
    for who in ["s3", "s5"] {
        let b = open(&w, "fake", who).await;
        let page = b.get("/app").await;
        assert_eq!(page.status, 200);
        assert!(
            page.body.contains("กลับไปที่แชทแล้วกด เริ่มใช้งาน"),
            "{who}: {}",
            page.body
        );
        assert!(!page.body.contains("ยอมรับประกาศความเป็นส่วนตัว"));
        assert!(!page.body.contains("ข่าวของฉัน"));
    }
    // An account the Deployment never saw gets the text without a session.
    let b = w.app.browser();
    let r = b.open_mini_app("fake", "fake-stranger", "/app").await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("กลับไปที่แชทแล้วกด เริ่มใช้งาน"));
    assert!(b.cookie("app_session").is_none());
}

#[tokio::test]
async fn a_session_ends_after_an_hour_and_the_entry_page_returns() {
    let w = boot().await;
    roster(&w).await;
    let b = open(&w, "fake", "s1").await;
    w.h.clock().advance_minutes(59);
    assert!(b.get("/app").await.body.contains("Name s1"));
    w.h.clock().advance_minutes(2);
    let page = b.get("/app").await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("id=\"xchg\""), "{}", page.body);
    assert!(!page.body.contains("Name s1"));
    // The row is gone and the cookie cleared.
    assert_eq!(count(&w, "SELECT count(*) FROM app_session").await, 0);
    assert!(b.cookie("app_session").is_none());
}

#[tokio::test]
async fn without_a_session_every_page_is_the_entry_page_for_its_own_path() {
    let w = boot().await;
    roster(&w).await;
    let b = w.app.browser();
    let page = b
        .get("/app/announcements/0193a0d2-0000-7000-8000-000000000001")
        .await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("action=\"/app/session\""));
    assert!(page.body.contains(
        "name=\"next\" value=\"/app/announcements/0193a0d2-0000-7000-8000-000000000001\""
    ));
    // The Fake Channels need no platform script.
    assert!(!page.body.contains("liff/edge/2/sdk.js"));
    assert!(!page.body.contains("data-liff"));
    assert!(!page.body.contains("telegram-web-app.js"));
    // The root forwards to /app with LIFF's deep path.
    let root = b.get("/?liff.state=%2Fapp%2Fannouncements").await;
    assert_eq!(root.status, 303);
    assert_eq!(
        root.header("location"),
        Some("/app?liff.state=%2Fapp%2Fannouncements")
    );
}

#[tokio::test]
async fn next_survives_the_exchange_and_foreign_targets_are_refused() {
    let w = boot().await;
    roster(&w).await;
    let deep = "/app/announcements/0193a0d2-0000-7000-8000-000000000001";
    let r = w.app.browser().open_mini_app("fake", "fake-s1", deep).await;
    assert_eq!(r.header("location"), Some(deep));
    for bad in [
        "https://evil.example/app",
        "//evil.example",
        "/admin",
        "/app/../admin",
    ] {
        w.h.clock().advance_ms(1_500);
        let r = w.app.browser().open_mini_app("fake", "fake-s1", bad).await;
        assert_eq!(r.status, 303, "{bad}");
        assert_eq!(r.header("location"), Some("/app"), "{bad}");
    }
}

#[tokio::test]
async fn one_session_per_account_per_second_and_ten_refusals_per_ip_per_minute() {
    let w = boot().await;
    roster(&w).await;
    assert_eq!(
        w.app
            .browser()
            .open_mini_app("fake", "fake-s1", "/app")
            .await
            .status,
        303
    );
    let again = w
        .app
        .browser()
        .open_mini_app("fake", "fake-s1", "/app")
        .await;
    assert_eq!(again.status, 429);
    w.h.clock().advance_ms(1_001);
    let later = w
        .app
        .browser()
        .open_mini_app("fake", "fake-s1", "/app")
        .await;
    assert_eq!(later.status, 303);
    // Both sessions work; the older one's expired siblings are removed
    // on the next exchange.
    assert_eq!(count(&w, "SELECT count(*) FROM app_session").await, 2);

    // Refusals are counted per client IP: the tenth in a minute is the
    // last one verified.
    let from = |ip: &'static str| {
        http::Request::post("/app/session")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("cf-connecting-ip", ip)
            .body(axum::body::Body::from("channel=fake&token=&next=%2Fapp"))
            .unwrap()
    };
    for _ in 0..10 {
        assert_eq!(w.app.request(from("203.0.113.9")).await.status, 403);
    }
    assert_eq!(w.app.request(from("203.0.113.9")).await.status, 429);
    assert_eq!(w.app.request(from("198.51.100.4")).await.status, 403);
    w.h.clock().advance_ms(61_000);
    assert_eq!(w.app.request(from("203.0.113.9")).await.status, 403);
}

#[tokio::test]
async fn the_hourly_job_prunes_sessions_expired_over_a_day_ago() {
    let w = boot().await;
    roster(&w).await;
    open(&w, "fake", "s1").await;
    // 23 h later another session starts; the first expired 22 h ago.
    w.h.clock().advance_minutes(23 * 60);
    open(&w, "fake", "s3").await;
    w.h.clock().advance_minutes(2 * 60 + 7);
    // 25:07 after the start: the first is over a day old, the second not.
    let now = w.h.clock().now_ms();
    w.app.deployment().tick(Timestamp(now)).await.unwrap();
    drain(&w).await;
    assert_eq!(
        texts(&w, "SELECT channel_account_id FROM app_session").await,
        ["a-fake-s3"]
    );
}
