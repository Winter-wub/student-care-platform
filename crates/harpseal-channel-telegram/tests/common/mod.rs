//! Shared setup: a Telegram Channel over a `RecordingHttp`.

#![allow(dead_code)]

use std::sync::Arc;

use harpseal::{ChannelAccountId, WebhookRequest};
use harpseal_channel_telegram::{KIND, SECRET_HEADER, TelegramChannel, TelegramConfig};
use harpseal_testing::{RecordedRequest, RecordingHttp};
use serde_json::Value;

pub const TOKEN: &str = "8000000001:SEALBAY";
pub const SECRET: &str = "seal-bay-webhook-secret";
pub const CHAT: &str = "7012345678";
pub const WEB_APP: &str = "https://app.sealbay.example/tg";

pub fn chat() -> ChannelAccountId {
    ChannelAccountId::new(KIND, CHAT)
}

/// A channel with no Web App, and its recorder.
pub fn channel() -> (TelegramChannel, RecordingHttp) {
    channel_with(TelegramConfig::new(TOKEN, SECRET, ""))
}

pub fn channel_with(cfg: TelegramConfig) -> (TelegramChannel, RecordingHttp) {
    let http = RecordingHttp::default();
    (TelegramChannel::new(cfg, Arc::new(http.clone())), http)
}

pub fn signed(body: &[u8], secret: Option<&str>) -> WebhookRequest {
    let mut headers = http::HeaderMap::new();
    if let Some(s) = secret {
        headers.insert(SECRET_HEADER, s.parse().unwrap());
    }
    WebhookRequest {
        headers,
        body: body.to_vec(),
    }
}

pub fn fixture(name: &str) -> WebhookRequest {
    let path = format!(
        "{}/tests/fixtures/webhook/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    signed(&std::fs::read(path).expect("fixture exists"), Some(SECRET))
}

/// A private text message from [`CHAT`].
pub fn text_update(text: &str) -> WebhookRequest {
    let body = serde_json::json!({ "update_id": 1, "message": {
        "message_id": 10, "date": 1_793_836_800,
        "from": { "id": 7_012_345_678_i64, "is_bot": false, "first_name": "Karn" },
        "chat": { "id": 7_012_345_678_i64, "type": "private" }, "text": text } });
    signed(body.to_string().as_bytes(), Some(SECRET))
}

/// The Bot API method of a request.
pub fn method(r: &RecordedRequest) -> &str {
    r.uri.rsplit('/').next().unwrap()
}

pub fn body(r: &RecordedRequest) -> Value {
    serde_json::from_slice(&r.body).unwrap()
}

/// `(method, body)` of every request so far, and forgets them.
pub fn calls(http: &RecordingHttp) -> Vec<(String, Value)> {
    http.take_requests()
        .iter()
        .map(|r| (method(r).to_string(), body(r)))
        .collect()
}

/// A Bot API success answer.
pub fn ok(result: Value) -> Vec<u8> {
    serde_json::json!({ "ok": true, "result": result })
        .to_string()
        .into_bytes()
}
