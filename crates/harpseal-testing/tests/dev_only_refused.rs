//! A dev-only Channel (the Fake) outside dev mode takes no input at all
//! (#133), whatever its secret: a correctly signed webhook is 404, a valid
//! Mini App token signs no one in, a session signed in while the
//! Deployment was in dev counts as none, and nothing is recorded or sent.
//! In dev mode the same calls work. One gate in the Core
//! (`Deployment::channel`) does it, so removing it fails these tests.

mod support;

use axum::body::Body;
use harpseal::{ActionId, Inbound, Statement};
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::{Harness, TestApp};
use http::Request;
use support::{Chat, ROSTER};

/// Tables an Inbound, a sign-in or a link flow writes to.
const RECORDS: &[&str] = &[
    "SELECT count(*) FROM channel_account",
    "SELECT count(*) FROM inbound_event",
    "SELECT count(*) FROM inbound_message",
    "SELECT count(*) FROM consent",
    "SELECT count(*) FROM verification_challenge",
    "SELECT count(*) FROM link",
    "SELECT count(*) FROM app_session",
    "SELECT count(*) FROM audit_log",
    "SELECT count(*) FROM security_event",
    "SELECT count(*) FROM outbound_message",
    "SELECT count(*) FROM rate_limit",
];

async fn records(app: &TestApp) -> Vec<i64> {
    let mut out = Vec::new();
    for sql in RECORDS {
        out.push(app.count(sql).await);
    }
    out
}

/// The harness Deployment, in dev mode or not, with the roster loaded.
async fn boot(h: &Harness, dev: bool) -> TestApp {
    let app = h.boot(h.builder().dev(dev).build().unwrap()).await;
    for sql in ROSTER {
        // A second boot on the same database finds the roster there.
        let _ = h.db().execute(Statement::new(*sql)).await;
    }
    app
}

/// Follow, consent, then the Student id: what a link flow starts with
/// (the last one emails the OTP).
fn link_flow_start(who: &str) -> Vec<Inbound> {
    let who = FakeChannel::account(who);
    let accept = ActionId::new("kernel.consent.accept").unwrap();
    vec![
        Inbound::follow(who.clone(), None),
        Inbound::action(who.clone(), accept, None),
        Inbound::text(who, "68010001", None),
    ]
}

#[tokio::test]
async fn outside_dev_mode_a_signed_fake_webhook_is_404_and_records_nothing() {
    let h = Harness::new();
    let app = boot(&h, false).await;
    let before = records(&app).await;
    for ev in link_flow_start("u1") {
        // Signed with the Channel's own secret: still refused.
        assert_eq!(app.fake_webhook([ev]).await.status, 404);
    }
    assert_eq!(records(&app).await, before, "nothing recorded");
    assert!(h.fake_channel().sent().is_empty(), "nothing sent");
    assert!(h.emails().is_empty(), "no OTP email");
    // The Core refuses it too when called directly.
    assert!(app.deployment().channel("fake").is_none());
}

#[tokio::test]
async fn in_dev_mode_the_same_webhook_starts_the_link_flow() {
    let h = Harness::new();
    let app = boot(&h, true).await;
    for ev in link_flow_start("u1") {
        assert_eq!(app.fake_webhook([ev]).await.status, 200);
    }
    assert_eq!(app.count("SELECT count(*) FROM channel_account").await, 1);
    assert!(!h.fake_channel().sent().is_empty());
    assert_eq!(h.emails().len(), 1, "the OTP email");
}

#[tokio::test]
async fn outside_dev_mode_a_valid_mini_app_token_signs_no_one_in() {
    let h = Harness::new();
    let dev = boot(&h, true).await;
    Chat::new(&h, &dev, "u1").link("68010001").await;
    let app = boot(&h, false).await;
    let before = records(&app).await;
    let b = app.browser();
    let r = b.open_mini_app(&h.fake_channel(), "u1", "/app").await;
    assert_eq!(r.status, 404, "{}", r.body);
    assert!(b.cookie("app_session").is_none());
    assert_eq!(records(&app).await, before, "nothing recorded");
    // In dev mode the same token signs the Student in.
    let b = dev.browser();
    let r = b.open_mini_app(&h.fake_channel(), "u1", "/app").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert!(b.get("/app").await.body.contains("Nok Thale"));
}

#[tokio::test]
async fn a_session_signed_in_during_dev_mode_counts_as_none_outside_it() {
    let h = Harness::new();
    let dev = boot(&h, true).await;
    Chat::new(&h, &dev, "u1").link("68010001").await;
    let b = dev.browser();
    let r = b.open_mini_app(&h.fake_channel(), "u1", "/app").await;
    assert_eq!(r.status, 303, "{}", r.body);
    let cookie = format!("app_session={}", b.cookie("app_session").unwrap());
    let app = boot(&h, false).await;
    let req = Request::get("/app")
        .header("cookie", cookie)
        .header("sec-fetch-site", "same-origin")
        .body(Body::empty())
        .unwrap();
    let r = app.request(req).await;
    assert_eq!(r.status, 200, "the entry page");
    assert!(!r.body.contains("Nok Thale"), "{}", r.body);
}

#[tokio::test]
async fn outside_dev_mode_nothing_is_sent_to_an_existing_fake_link() {
    let h = Harness::new();
    let dev = boot(&h, true).await;
    Chat::new(&h, &dev, "u1").link("68010001").await;
    let app = boot(&h, false).await;
    h.fake_channel().take_sent();
    let to = FakeChannel::account("u1");
    let sent = app
        .deployment()
        .send(
            &to,
            None,
            &harpseal::Message::text("hi"),
            &harpseal::Locale::new("th"),
        )
        .await;
    assert!(sent.is_err(), "{sent:?}");
    assert!(h.fake_channel().sent().is_empty());
}
