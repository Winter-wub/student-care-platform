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
async fn a_signed_text_is_stored_and_answered_with_one_fallback() {
    // Free text after Link: stored, answered once per hour with the
    // fallback Text + Menu Choices in one reply call.
    let (h, app) = linked().await;
    let body = fixture_body("text");
    let resp = post(&app, body.clone(), &sign(SECRET, &body)).await;
    assert_eq!(resp.status, 200);
    let calls = h.http().take_requests();
    assert_eq!(calls.len(), 1, "one reply call");
    assert!(calls[0].uri.ends_with("/message/reply"), "{}", calls[0].uri);
    assert_eq!(app.count("SELECT count(*) FROM inbound_message").await, 1);
    // A new event (another webhookEventId) with the same text.
    let body = String::from_utf8(body)
        .unwrap()
        .replace("01JSEALBAY0000000000000006", "01JSEALBAY0000000000000099")
        .into_bytes();
    let resp = post(&app, body.clone(), &sign(SECRET, &body)).await;
    assert_eq!(resp.status, 200);
    assert!(h.http_requests().is_empty(), "no second fallback this hour");
    assert_eq!(app.count("SELECT count(*) FROM inbound_message").await, 2);
}

#[tokio::test]
async fn a_redelivered_event_is_handled_once_and_never_falls_back_to_push() {
    // LINE redelivers the text event (same webhookEventId, isRedelivery
    // true) after the first delivery already spent the reply token. Before
    // de-duplication the spent token fell back to a metered Push.
    let (h, app) = linked().await;
    let body = fixture_body("text");
    let resp = post(&app, body.clone(), &sign(SECRET, &body)).await;
    assert_eq!(resp.status, 200);
    assert_eq!(h.http().take_requests().len(), 1, "one reply call");
    let sent = app.count("SELECT count(*) FROM outbound_message").await;
    let again = String::from_utf8(body)
        .unwrap()
        .replace(r#""isRedelivery": false"#, r#""isRedelivery": true"#)
        .into_bytes();
    h.http()
        .respond_with(400, r#"{"message":"Invalid reply token"}"#);
    let resp = post(&app, again.clone(), &sign(SECRET, &again)).await;
    assert_eq!(resp.status, 200);
    assert!(h.http_requests().is_empty(), "no reply, no Push");
    assert_eq!(app.count("SELECT count(*) FROM inbound_message").await, 1);
    assert_eq!(
        app.count("SELECT count(*) FROM outbound_message").await,
        sent
    );
    let metered = app
        .count("SELECT coalesce(sum(metered), 0) FROM usage_daily WHERE channel = 'line'")
        .await;
    assert_eq!(metered, 0);
    let claimed = app
        .count("SELECT count(*) FROM inbound_event WHERE channel = 'line'")
        .await;
    assert!(claimed >= 1);
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

#[tokio::test]
async fn ten_identical_texts_are_two_pushes_with_different_retry_keys() {
    // Two identical 5-object Pushes in one batch: with one shared retry key
    // LINE would answer 409 to the second and the Student would get 5.
    let (h, app) = linked().await;
    let to = harpseal::ChannelAccountId::new(harpseal_channel_line::KIND, USER);
    let msgs = vec![
        Message::text("ประกาศ: หยุดเรียนวันศุกร์");
        10
    ];
    let run = || async {
        app.deployment()
            .send_batch_with_key(&to, None, &msgs, &harpseal::Locale::th(), "job-3/a1")
            .await
            .unwrap();
        let reqs = h.http().take_requests();
        assert_eq!(reqs.len(), 2);
        assert_eq!(json(&reqs[0].body), json(&reqs[1].body), "identical");
        reqs.iter()
            .map(|r| r.headers[harpseal_channel_line::RETRY_KEY_HEADER].clone())
            .collect::<Vec<_>>()
    };
    let keys = run().await;
    assert_ne!(keys[0], keys[1]);
    assert_eq!(run().await, keys, "a retry of the whole batch reuses them");
    let metered = app
        .count("SELECT coalesce(sum(metered), 0) FROM usage_daily WHERE channel = 'line'")
        .await;
    assert_eq!(metered, 4, "two Push requests per send");
}

/// 61 Cards: 6 carousels, so Push requests of 5 and 1.
fn cards61() -> Message {
    Message::CardList(harpseal::CardList(
        (0..61)
            .map(|i| harpseal::Card {
                title: format!("ประกาศ {i}"),
                body: "มหาวิทยาลัยอ่าวแมวน้ำ".into(),
                buttons: Vec::new(),
            })
            .collect(),
    ))
}

#[tokio::test]
async fn a_resume_after_a_partial_send_meters_the_409_push_once_and_keeps_one_row() {
    // #44: push 1 of a Message went out and was recorded with 1 unit; the
    // resume rebuilds it, LINE answers 409, and it must not count again.
    let (h, app) = linked().await;
    let to = harpseal::ChannelAccountId::new(harpseal_channel_line::KIND, USER);
    let msgs = [cards61()];
    let d = app.deployment();
    let th = harpseal::Locale::th();
    let send = || d.send_batch_with_key(&to, None, &msgs, &th, "job-9/a1");
    h.http().respond_with(200, "{}");
    h.http().respond_with(500, r#"{"message":"boom"}"#);
    let err = send().await.unwrap_err();
    assert!(
        matches!(err, harpseal::Error::PartialSend { sent: 0, .. }),
        "{err}"
    );
    const METERED: &str =
        "SELECT coalesce(sum(metered), 0) FROM usage_daily WHERE channel = 'line'";
    assert_eq!(app.count(METERED).await, 1);
    h.http()
        .respond_with(409, r#"{"message":"The retry key is already accepted"}"#);
    h.http().respond_with(200, "{}");
    let receipts = send().await.unwrap();
    assert_eq!(receipts[0].metered_units, 2);
    assert_eq!(app.count(METERED).await, 2, "two Push requests, each once");
    assert_eq!(
        app.count("SELECT count(*) FROM outbound_message").await,
        1,
        "the partial row is completed, not doubled"
    );
    let row = app
        .query("SELECT metering, error FROM outbound_message")
        .await;
    assert_eq!(row.get(0, "metering").unwrap().as_text(), Some("metered"));
    assert_eq!(
        row.get(0, "error"),
        Some(&harpseal::Value::Null),
        "complete now"
    );
}
