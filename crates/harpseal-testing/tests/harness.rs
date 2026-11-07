//! The harness itself and the Kernel behaviour this ticket ships, driven
//! through the Router the way a platform or browser would.

use axum::body::Body;
use harpseal::{
    ActionContext, ActionId, CatalogEntry, Inbound, Message, Module, ModuleRegistrar, ReplyToken,
    Statement, Timestamp,
};
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::{Harness, START_MS};
use http::Request;

fn is_uuid_v7(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() == 36
        && [8, 13, 18, 23].iter().all(|&i| b[i] == b'-')
        && b[14] == b'7'
        && matches!(b[19], b'8' | b'9' | b'a' | b'b')
        && s.chars()
            .all(|c| c == '-' || c.is_ascii_digit() || ('a'..='f').contains(&c))
}

#[test]
fn ids_are_uuid_v7_from_the_clock_and_random_ports() {
    let h = Harness::new();
    let s = h.services();
    let a = s.new_id().unwrap();
    assert!(is_uuid_v7(&a), "{a}");
    // The first 48 bits are the Clock's milliseconds.
    let ms = i64::from_str_radix(&a[..13].replace('-', ""), 16).unwrap();
    assert_eq!(ms, START_MS);
    h.clock().advance_ms(1);
    let b = s.new_id().unwrap();
    assert!(a < b, "ids sort by creation time");
    // Deterministic Random: a fresh harness repeats the same ids.
    assert_eq!(Harness::new().services().new_id().unwrap(), a);
}

#[test]
fn the_clock_only_moves_when_told() {
    let h = Harness::new();
    assert_eq!(h.services().now_ms(), START_MS);
    h.clock().advance_minutes(30);
    assert_eq!(h.services().now_ms(), START_MS + 30 * 60_000);
    h.clock().set_ms(5);
    assert_eq!(h.services().now_ms(), 5);
}

#[tokio::test]
async fn email_and_http_are_captured_not_sent() {
    let h = Harness::new();
    let s = h.services();
    let email = harpseal::Email {
        to: "s68010001@student.sealbay.example".into(),
        subject: "รหัสยืนยัน".into(),
        text: "123456".into(),
        html: None,
    };
    s.email.send(email.clone()).await.unwrap();
    assert_eq!(h.emails(), vec![email]);

    h.http().respond_with(429, "slow down");
    let req = http::Request::post("https://api.example.invalid/v1")
        .body(b"{\"a\":1}".to_vec())
        .unwrap();
    let resp = s.http.send(req).await.unwrap();
    assert_eq!(resp.status(), 429);
    let again = s
        .http
        .send(
            http::Request::get("https://api.example.invalid/v2")
                .body(vec![])
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        again.status(),
        200,
        "default answer once the queue is empty"
    );
    let seen = h.http_requests();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].body_text(), "{\"a\":1}");
}

#[tokio::test]
async fn notice_page_is_public() {
    let app = Harness::new().start().await;
    let r = app.get("/notice").await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("ประกาศความเป็นส่วนตัว"), "{}", r.body);
    assert!(r.header("content-type").unwrap().starts_with("text/html"));
}

#[tokio::test]
async fn stylesheet_is_served() {
    let app = Harness::new().start().await;
    let r = app.get("/static/app.css").await;
    assert_eq!(r.status, 200);
    assert!(r.header("content-type").unwrap().starts_with("text/css"));
}

struct Reports;

impl Module for Reports {
    fn name(&self) -> &'static str {
        "reports"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.permission(harpseal::Permission::new("reports.view", "See reports"))
            .back_office(
                "reports.view",
                "/reports",
                axum::routing::get(|| async { "secret" }),
            );
    }
}

#[tokio::test]
async fn back_office_routes_refuse_requests_without_a_staff_session() {
    let h = Harness::new();
    let app = h.boot(h.builder().module(Reports).build().unwrap()).await;
    let r = app.get("/admin/reports").await;
    assert_eq!(r.status, 303);
    assert_eq!(r.header("location"), Some("/admin/login"));
    assert!(!r.body.contains("secret"));
}

#[tokio::test]
async fn webhook_with_a_bad_signature_is_rejected_before_parsing() {
    let h = Harness::new();
    let app = h.start().await;
    let fake = h.fake_channel();
    let body = FakeChannel::encode(&[Inbound::follow(FakeChannel::account("u1"), None)]);
    assert_eq!(
        app.fake_webhook_raw(body.clone(), "wrong").await.status,
        401
    );
    assert_eq!(app.fake_webhook_raw(body, "").await.status, 401);
    // A correct signature over a body that is not JSON: 400, not 401.
    let garbage = b"{not json".to_vec();
    let sig = fake.sign(&garbage);
    assert_eq!(app.fake_webhook_raw(garbage, &sig).await.status, 400);
    assert!(fake.sent().is_empty());
    assert_eq!(app.get("/webhook/nope").await.status, 405);
    let unknown = Request::post("/webhook/nope").body(Body::empty()).unwrap();
    assert_eq!(app.request(unknown).await.status, 404);
}

fn press(account: &str, action: &str, reply_expires_at: i64) -> Inbound {
    Inbound::action(
        FakeChannel::account(account),
        ActionId::new(action).unwrap(),
        Some(ReplyToken {
            token: "r-1".into(),
            expires_at: Timestamp(reply_expires_at),
        }),
    )
}

#[tokio::test]
async fn an_unlinked_account_is_greeted_with_the_consent_card() {
    let h = Harness::new();
    let app = h.start().await;
    let r = app
        .fake_webhook([press("u1", "news.latest", START_MS + 1)])
        .await;
    assert_eq!(r.status, 200);
    let sent = h.fake_channel().sent();
    assert_eq!(sent.len(), 2, "welcome Text + Consent Card");
    assert!(matches!(&sent[1].message, Message::Card(c) if c.title == "ประกาศความเป็นส่วนตัว"));
    assert_eq!(
        sent[0].metering,
        harpseal::Metering::Free,
        "answered as a Reply"
    );
}

struct Hello;

impl Module for Hello {
    fn name(&self) -> &'static str {
        "hello"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.messages(&[
            CatalogEntry::new("hello.hi", "th", "สวัสดี"),
            CatalogEntry::new("hello.hi", "en", "Hello"),
        ])
        .action(
            ActionId::new("hello.say").unwrap(),
            |cx: ActionContext| async move { Ok(vec![Message::text(cx.t("hello.hi"))]) },
        );
    }
}

#[tokio::test]
async fn a_linked_student_reaches_the_module_in_their_locale() {
    let h = Harness::new();
    let app = h.boot(h.builder().module(Hello).build().unwrap()).await;
    let setup = [
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('s1', '68010004', 'Karn Bai-ruea', 'en', 1, 1)",
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
         VALUES ('a1', 'fake', 'u1', 1, 1)",
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, linked_at, updated_at) \
         VALUES ('l1', 's1', 'a1', 'fake', 'active', 'email_otp', 1, 1)",
    ];
    for sql in setup {
        h.db().execute(Statement::new(sql)).await.unwrap();
    }
    // The reply token has expired, so the answer goes out as a metered Push.
    app.fake_webhook([press("u1", "hello.say", START_MS)]).await;
    app.fake_webhook([press("u1", "gone.away", START_MS)]).await;
    let seen = h.fake_channel().sent_to(&FakeChannel::account("u1"));
    assert!(matches!(&seen[0], Message::Text(t) if t == "Hello"));
    assert!(matches!(&seen[1], Message::Text(t) if t.starts_with("This button no longer works")));
    assert_eq!(
        h.fake_channel().sent()[0].metering,
        harpseal::Metering::Metered
    );
}
