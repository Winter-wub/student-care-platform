//! The Channel contract suite against the Telegram adapter, through a
//! `RecordingHttp` (nothing leaves the process).
//!
//! `#[ignore]` until #17: the adapter's update parsing and Bot API calls
//! still return `Error::Platform`. #17 removes the `#[ignore]`; the
//! recorded updates and the wire reader are ready.

use std::sync::Arc;

use harpseal::{ActionId, ChannelAccountId, Delivery, Metering, WebhookRequest};
use harpseal_channel_telegram::{KIND, TelegramChannel, TelegramConfig, metering};
use harpseal_testing::RecordingHttp;
use harpseal_testing::contract::{Fixtures, Target, Wire, run};
use serde_json::{Value, json};

const SECRET: &str = "seal-bay-webhook-secret";
const HEADER: &str = "x-telegram-bot-api-secret-token";
const CHAT: &str = "7012345678";

fn signed(body: &[u8], secret: &str) -> WebhookRequest {
    let mut headers = http::HeaderMap::new();
    headers.insert(HEADER, secret.parse().unwrap());
    WebhookRequest {
        headers,
        body: body.to_vec(),
    }
}

fn fixture(name: &str) -> WebhookRequest {
    let path = format!(
        "{}/tests/fixtures/webhook/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    signed(&std::fs::read(path).expect("fixture exists"), SECRET)
}

/// `callback_data` in every Bot API call made.
struct TelegramWire(RecordingHttp);

fn callbacks(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            if let Some(d) = o.get("callback_data").and_then(Value::as_str) {
                out.push(d.to_string());
            }
            o.values().for_each(|x| callbacks(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| callbacks(x, out)),
        _ => {}
    }
}

impl Wire for TelegramWire {
    fn take_action_payloads(&self) -> Vec<String> {
        let mut out = Vec::new();
        for r in self.0.take_requests() {
            if let Ok(v) = serde_json::from_slice::<Value>(&r.body) {
                callbacks(&v, &mut out);
            }
        }
        out
    }
    fn press(&self, from: &ChannelAccountId, payload: &str) -> WebhookRequest {
        let id: i64 = from.external_id.parse().unwrap_or(0);
        let body = json!({ "update_id": 900_000_100, "callback_query": {
            "id": "press-1", "from": { "id": id, "is_bot": false, "first_name": "Karn" },
            "message": { "message_id": 9, "date": 1_793_836_800,
                         "chat": { "id": id, "type": "private" }, "text": "x" },
            "chat_instance": "-1", "data": payload,
        }});
        signed(body.to_string().as_bytes(), SECRET)
    }
    fn metering(&self, d: &Delivery) -> Metering {
        metering(d)
    }
}

fn target() -> Target {
    let http = RecordingHttp::default();
    let cfg = TelegramConfig::new("8000000001:SEALBAY", SECRET, "");
    let channel = TelegramChannel::new(cfg, Arc::new(http.clone()));
    let fixtures = Fixtures::new(
        fixture("start"),
        fixture("text"),
        fixture("document"),
        (
            fixture("callback_query"),
            ActionId::new("news.ack.1042").unwrap(),
        ),
        fixture("kicked"),
        fixture("member"),
        signed(b"{not json", "wrong-secret"),
        signed(b"{not json", SECRET),
        ChannelAccountId::new(KIND, CHAT),
    );
    Target::new(Arc::new(channel), fixtures, Arc::new(TelegramWire(http)))
}

#[tokio::test]
#[ignore = "Telegram update parsing and Bot API calls arrive with #17; remove this then"]
async fn telegram_passes_the_channel_contract() {
    run(target).await;
}
