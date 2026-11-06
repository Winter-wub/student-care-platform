//! The LINE adapter at the Deployment seam: a Deployment built with the
//! LINE Channel on the harness's RecordingHttp, driven through its Router
//! with signed webhooks, as LINE would call it.

mod common;

use std::sync::Arc;

use axum::body::Body;
use common::{SECRET, TOKEN, USER, fixture_body, json};
use harpseal::{ActionContext, ActionId, Message, Module, ModuleRegistrar, Statement};
use harpseal_channel_line::{LineChannel, LineConfig, SIGNATURE_HEADER, sign};
use harpseal_testing::{Harness, TestApp, TestResponse};
use serde_json::json;

/// Answers the `news.ack.1042` press of the postback fixture with two
/// messages.
struct News;

impl Module for News {
    fn name(&self) -> &'static str {
        "news"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.action(
            ActionId::new("news.ack.1042").unwrap(),
            |_: ActionContext| async move {
                Ok(vec![
                    Message::text("รับทราบประกาศแล้ว ขอบคุณครับ"),
                    harpseal::fixtures::news_card(),
                ])
            },
        );
    }
}

/// A Deployment with the LINE Channel and the News Module; the fixtures'
/// LINE user is linked to a Student.
async fn linked() -> (Harness, TestApp) {
    let h = Harness::new();
    let line = LineChannel::new(
        LineConfig::new(SECRET, TOKEN, ""),
        Arc::new(h.http().clone()),
    );
    let d = h.builder().channel(line).module(News).build().unwrap();
    let app = h.boot(d).await;
    let account = format!(
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
         VALUES ('a1', 'line', '{USER}', 1, 1)"
    );
    for sql in [
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('s1', '68010004', 'Karn Bai-ruea', 'th', 1, 1)"
            .to_string(),
        account,
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, linked_at, updated_at) \
         VALUES ('l1', 's1', 'a1', 'line', 'active', 'email_otp', 1, 1)"
            .to_string(),
    ] {
        h.db().execute(Statement::new(sql)).await.unwrap();
    }
    (h, app)
}

async fn post(app: &TestApp, body: Vec<u8>, signature: &str) -> TestResponse {
    let req = http::Request::post("/webhook/line")
        .header(SIGNATURE_HEADER, signature)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    app.request(req).await
}

#[tokio::test]
async fn a_signed_postback_is_answered_with_one_free_reply() {
    let (h, app) = linked().await;
    let body = fixture_body("postback");
    let resp = post(&app, body.clone(), &sign(SECRET, &body)).await;
    assert_eq!(resp.status, 200);

    let reqs = h.http_requests();
    assert_eq!(reqs.len(), 1, "one call: {reqs:#?}");
    assert_eq!(reqs[0].uri, "https://api.line.me/v2/bot/message/reply");
    let sent = json(&reqs[0].body);
    assert_eq!(sent["replyToken"], "d81f654086166f29b18f0a1fa8e1626d");
    let messages = sent["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2, "the handler's two messages");
    assert_eq!(
        messages[0],
        json!({ "type": "text", "text": "รับทราบประกาศแล้ว ขอบคุณครับ" })
    );
    assert_eq!(messages[1]["type"], "flex");

    let rows = app
        .query(
            "SELECT channel, push, metering, module, kind FROM outbound_message ORDER BY kind DESC",
        )
        .await;
    assert_eq!(rows.len(), 2, "one row per handler Message");
    assert_eq!(rows.get(0, "kind").unwrap().as_text(), Some("text"));
    for i in 0..2 {
        assert_eq!(rows.get(i, "channel").unwrap().as_text(), Some("line"));
        assert_eq!(rows.get(i, "metering").unwrap().as_text(), Some("free"));
        assert_eq!(rows.get(i, "push").unwrap().as_i64(), Some(0));
        assert_eq!(rows.get(i, "module").unwrap().as_text(), Some("news"));
    }
    let metered = app
        .count("SELECT coalesce(sum(metered), 0) FROM usage_daily WHERE channel = 'line'")
        .await;
    assert_eq!(metered, 0);
}

#[tokio::test]
async fn a_signed_text_is_accepted_and_sends_nothing_yet() {
    // The Kernel answers free text with #19; until then nothing goes out.
    let (h, app) = linked().await;
    let body = fixture_body("text");
    let resp = post(&app, body.clone(), &sign(SECRET, &body)).await;
    assert_eq!(resp.status, 200);
    assert!(h.http_requests().is_empty());
}

#[tokio::test]
async fn a_bad_signature_is_401_and_nothing_happens() {
    let (h, app) = linked().await;
    let body = fixture_body("postback");
    let resp = post(&app, body.clone(), &sign("not-the-secret", &body)).await;
    assert_eq!(resp.status, 401);
    let resp = post(&app, b"{not json".to_vec(), &sign(SECRET, b"{not json")).await;
    assert_eq!(resp.status, 400);
    assert!(h.http_requests().is_empty());
    assert_eq!(app.count("SELECT count(*) FROM outbound_message").await, 0);
}
