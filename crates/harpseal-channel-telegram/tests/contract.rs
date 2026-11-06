//! The Channel contract suite against the Telegram adapter, through a
//! `RecordingHttp` (nothing leaves the process). The wire reader also
//! checks the JSON of every Bot API call it reads.

use std::sync::Arc;

use harpseal::{ActionId, ChannelAccountId, Delivery, Metering, WebhookRequest};
use harpseal_channel_telegram::{KIND, TelegramChannel, TelegramConfig, metering};
use harpseal_testing::contract::{Call, Fixtures, Target, Wire, WireLog, run};
use harpseal_testing::{RecordedRequest, RecordingHttp};
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

fn json(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap_or(Value::Null)
}

/// Bot API methods that send one message each.
const SENDS: &[&str] = &["/sendMessage", "/sendDocument", "/sendPhoto"];

const BOT: &str = "https://api.telegram.org/bot8000000001:SEALBAY/";

/// Every call is a JSON `POST` to the bot's URL; a send names the chat and
/// has a text or a file.
fn assert_request(r: &RecordedRequest) {
    assert_eq!(r.method, http::Method::POST, "{}", r.uri);
    assert!(r.uri.starts_with(BOT), "{}", r.uri);
    assert_eq!(r.headers["content-type"], "application/json");
    let body = json(&r.body);
    assert!(body.is_object(), "{}: JSON object body", r.uri);
    if SENDS.iter().any(|m| r.uri.ends_with(m)) {
        assert_eq!(body["chat_id"], CHAT, "{}", r.uri);
        assert!(body.get("method").is_none(), "method is in the URL");
        let content = ["text", "document", "photo"];
        assert!(content.iter().any(|k| body[k].is_string()), "{body}");
    }
    if r.uri.ends_with("/answerCallbackQuery") {
        assert!(body["callback_query_id"].is_string(), "{body}");
    }
}

impl Wire for TelegramWire {
    fn take(&self) -> WireLog {
        let mut payloads = Vec::new();
        let mut calls = Vec::new();
        for r in self.0.take_requests() {
            assert_request(&r);
            if SENDS.iter().any(|m| r.uri.ends_with(m)) {
                calls.push(Call::push(1));
                callbacks(&json(&r.body), &mut payloads);
            }
        }
        WireLog::new(payloads, calls)
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
    fn reply_limit(&self) -> Option<usize> {
        None
    }
    /// The last `setMyCommands` scoped to this chat decides; `/start`
    /// alone is the locked Menu. Each one is followed by a
    /// `setChatMenuButton` for the same chat.
    fn menu_entries(&self, to: &ChannelAccountId) -> Option<usize> {
        let reqs = self.0.requests();
        let last = reqs.iter().rev().find(|r| {
            (r.uri.ends_with("/setMyCommands") || r.uri.ends_with("/deleteMyCommands"))
                && json(&r.body)["scope"]["chat_id"]
                    .to_string()
                    .trim_matches('"')
                    == to.external_id
        })?;
        let button = reqs
            .last()
            .filter(|r| r.uri.ends_with("/setChatMenuButton"));
        let button = json(&button.expect("a menu button call last").body);
        assert_eq!(button["chat_id"], CHAT);
        assert_eq!(button["menu_button"]["type"], "commands");
        if last.uri.ends_with("/deleteMyCommands") {
            return None;
        }
        let commands = json(&last.body)["commands"].as_array().cloned()?;
        let locked = commands.len() == 1 && commands[0]["command"] == "start";
        (!locked && !commands.is_empty()).then_some(commands.len())
    }
}

fn target() -> Target {
    let http = RecordingHttp::default();
    let cfg = TelegramConfig::new("8000000001:SEALBAY", SECRET, "");
    let channel = TelegramChannel::new(cfg, Arc::new(http.clone()));
    let start = fixture("start");
    let mut missing = start.clone();
    missing.headers.remove(HEADER);
    let fixtures = Fixtures::builder(ChannelAccountId::new(KIND, CHAT))
        .follow(start.clone())
        .text(fixture("text"))
        .file(fixture("document"))
        .action(
            fixture("callback_query"),
            ActionId::new("news.ack.1042").unwrap(),
        )
        .blocked(fixture("kicked"))
        .unblocked(fixture("member"))
        .bad_signature(signed(b"{not json", "wrong-secret"))
        .bad_signature_valid_body(signed(&start.body, "wrong-secret"))
        .missing_signature(missing)
        .signed_garbage(signed(b"{not json", SECRET))
        .build();
    Target::new(Arc::new(channel), fixtures, Arc::new(TelegramWire(http)))
}

#[tokio::test]
async fn telegram_passes_the_channel_contract() {
    run(target).await;
}
