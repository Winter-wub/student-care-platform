//! The Channel contract suite against the LINE adapter, through a
//! `RecordingHttp` (nothing leaves the process).
//!
//! `#[ignore]` until #16: the adapter's webhook parsing and HTTP send still
//! return `Error::Platform`. #16 removes the `#[ignore]`; the fixtures
//! (recorded webhook bodies signed with HMAC-SHA256 below) and the wire
//! reader are ready.

use std::sync::Arc;

use harpseal::{ActionId, ChannelAccountId, Delivery, Metering, WebhookRequest};
use harpseal_channel_line::{KIND, LineChannel, LineConfig, metering};
use harpseal_testing::RecordingHttp;
use harpseal_testing::contract::{Call, Fixtures, Target, Wire, WireLog, run};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SECRET: &str = "seal-bay-line-channel-secret";
const USER: &str = "U5ea1ba7000000000000000000000c0de";

/// HMAC-SHA256 (RFC 2104) from the plain hash, as LINE signs webhooks.
fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let pad = |b: u8| k.iter().map(|x| x ^ b).collect::<Vec<u8>>();
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(msg)
        .finalize();
    Sha256::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, b)| a | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn signed(body: &[u8], secret: &str) -> WebhookRequest {
    let mut headers = http::HeaderMap::new();
    let sig = base64(&hmac_sha256(secret.as_bytes(), body));
    headers.insert("x-line-signature", sig.parse().unwrap());
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

/// Postback `data` in every request body LINE received.
struct LineWire(RecordingHttp);

fn postbacks(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            if o.get("type").and_then(Value::as_str) == Some("postback")
                && let Some(d) = o.get("data").and_then(Value::as_str)
            {
                out.push(d.to_string());
            }
            o.values().for_each(|x| postbacks(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| postbacks(x, out)),
        _ => {}
    }
}

fn json(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap_or(Value::Null)
}

impl Wire for LineWire {
    fn take(&self) -> WireLog {
        let mut payloads = Vec::new();
        let mut calls = Vec::new();
        for r in self.0.take_requests() {
            let v = json(&r.body);
            let objects = v["messages"].as_array().map_or(0, Vec::len);
            if r.uri.ends_with("/v2/bot/message/reply") {
                let token = v["replyToken"].as_str().unwrap_or_default();
                calls.push(Call::reply(token, objects));
            } else if r.uri.ends_with("/v2/bot/message/push") {
                calls.push(Call::push(objects));
            } else {
                continue; // rich menu calls carry Menu postbacks, not messages
            }
            postbacks(&v, &mut payloads);
        }
        WireLog::new(payloads, calls)
    }
    fn press(&self, from: &ChannelAccountId, payload: &str) -> WebhookRequest {
        let body = json!({ "destination": "Useal0bay0bot", "events": [{
            "type": "postback", "mode": "active", "timestamp": 1_793_836_800_000_i64,
            "webhookEventId": "01JSEALBAYPRESS", "deliveryContext": { "isRedelivery": false },
            "source": { "type": "user", "userId": from.external_id },
            "replyToken": "f0press", "postback": { "data": payload },
        }]});
        signed(body.to_string().as_bytes(), SECRET)
    }
    fn metering(&self, d: &Delivery) -> Metering {
        metering(d)
    }
    fn reply_limit(&self) -> Option<usize> {
        Some(5)
    }
    /// The last per-user rich menu call decides: an unlink (DELETE) is the
    /// locked Menu; a link shows the areas of the last rich menu created.
    /// #16 may adjust this to the rich menu flow it implements.
    fn menu_entries(&self, to: &ChannelAccountId) -> Option<usize> {
        let reqs = self.0.requests();
        let user = format!("/v2/bot/user/{}/richmenu", to.external_id);
        let last = reqs.iter().rev().find(|r| r.uri.contains(&user))?;
        if last.method == http::Method::DELETE {
            return None;
        }
        let created = reqs
            .iter()
            .rev()
            .find(|r| r.uri.ends_with("/v2/bot/richmenu"))?;
        json(&created.body)["areas"].as_array().map(Vec::len)
    }
}

fn target() -> Target {
    let http = RecordingHttp::default();
    let cfg = LineConfig::new(SECRET, "seal-bay-access-token", "");
    let channel = LineChannel::new(cfg, Arc::new(http.clone()));
    let follow = fixture("follow");
    let mut missing = follow.clone();
    missing.headers.remove("x-line-signature");
    let fixtures = Fixtures::builder(ChannelAccountId::new(KIND, USER))
        .follow(follow.clone())
        .text(fixture("text"))
        .file(fixture("file"))
        .action(fixture("postback"), ActionId::new("news.ack.1042").unwrap())
        .blocked(fixture("unfollow"))
        .unblocked(fixture("refollow"))
        .bad_signature(signed(b"{not json", "wrong-secret"))
        .bad_signature_valid_body(signed(&follow.body, "wrong-secret"))
        .missing_signature(missing)
        .signed_garbage(signed(b"{not json", SECRET))
        .build();
    Target::new(Arc::new(channel), fixtures, Arc::new(LineWire(http)))
}

#[test]
fn the_hmac_matches_rfc_4231() {
    // RFC 4231 test case 2.
    let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
    let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        hex,
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert_eq!(base64(b"Ma"), "TWE=");
}

#[tokio::test]
#[ignore = "LINE webhook parsing and HTTP send arrive with #16; remove this then"]
async fn line_passes_the_channel_contract() {
    run(target).await;
}
