//! The Channel contract suite against the LINE adapter, through a
//! recording HttpClient that answers like the Messaging API (nothing
//! leaves the process). The fixtures are recorded webhook bodies signed
//! here with an HMAC-SHA256 written from RFC 2104, independent of the
//! adapter's.

mod common;

use std::sync::Arc;

use common::{LineApi, SECRET, TOKEN, USER, fixture, json, png};
use harpseal::{ActionId, ChannelAccountId, Delivery, Menu, MenuEntry, Metering, WebhookRequest};
use harpseal_channel_line::{KIND, LineChannel, LineConfig, metering};
use harpseal_testing::contract::{Call, Fixtures, Target, Wire, WireLog, run};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

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

/// Signed with the RFC 2104 construction above, not the adapter's code.
fn signed(body: &[u8], secret: &str) -> WebhookRequest {
    let mut headers = http::HeaderMap::new();
    let sig = base64(&hmac_sha256(secret.as_bytes(), body));
    headers.insert("x-line-signature", sig.parse().unwrap());
    WebhookRequest {
        headers,
        body: body.to_vec(),
    }
}

/// The locked Menu of the contract target.
const LOCKED_ACTION: &str = "contract.locked.start";

fn locked_menu() -> Menu {
    Menu {
        entries: vec![MenuEntry {
            label: "เริ่มใช้งาน".into(),
            action: ActionId::new(LOCKED_ACTION).unwrap(),
        }],
    }
}

/// Reads the requests LINE received.
struct LineWire(LineApi);

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

impl Wire for LineWire {
    fn take(&self) -> WireLog {
        let mut payloads = Vec::new();
        let mut calls = Vec::new();
        for r in self.0.take() {
            let v = json(&r.body);
            assert_eq!(
                r.headers["authorization"],
                format!("Bearer {TOKEN}").as_str()
            );
            let objects = v["messages"].as_array().map_or(0, Vec::len);
            if r.uri == "https://api.line.me/v2/bot/message/reply" {
                let token = v["replyToken"].as_str().unwrap_or_default();
                calls.push(Call::reply(token, objects));
            } else if r.uri == "https://api.line.me/v2/bot/message/push" {
                assert_eq!(v["to"], USER, "push goes to the account");
                calls.push(Call::push(objects));
            } else {
                continue; // rich menu calls carry Menu postbacks, not messages
            }
            assert!((1..=5).contains(&objects), "1..=5 objects per request");
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
    /// The last per-user rich menu call decides: an unlink (DELETE), or a
    /// link to the locked Menu's rich menu, is the locked Menu; a link to
    /// any other rich menu shows its areas.
    fn menu_entries(&self, to: &ChannelAccountId) -> Option<usize> {
        let reqs = self.0.requests();
        let user = format!(
            "https://api.line.me/v2/bot/user/{}/richmenu",
            to.external_id
        );
        let last = reqs.iter().rev().find(|r| r.uri.starts_with(&user))?;
        if last.method == http::Method::DELETE {
            return None;
        }
        let id = last.uri.rsplit('/').next()?;
        let menu = self.0.created(id)?;
        let mut data = Vec::new();
        postbacks(&menu, &mut data);
        (data != [LOCKED_ACTION]).then_some(data.len())
    }
}

fn target() -> Target {
    let api = LineApi::default();
    let cfg = LineConfig::new(SECRET, TOKEN, "")
        .with_locked_menu(locked_menu())
        .with_menu_images(png(2500, 843), png(2500, 1686));
    let channel = LineChannel::new(cfg, Arc::new(api.clone()));
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
    Target::new(Arc::new(channel), fixtures, Arc::new(LineWire(api)))
}

#[test]
fn the_hmac_matches_rfc_4231_and_the_adapter() {
    // RFC 4231 test case 2.
    let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
    let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        hex,
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert_eq!(base64(b"Ma"), "TWE=");
    let body = b"{\"events\":[]}";
    assert_eq!(
        base64(&hmac_sha256(SECRET.as_bytes(), body)),
        harpseal_channel_line::sign(SECRET, body)
    );
}

#[tokio::test]
async fn line_passes_the_channel_contract() {
    run(target).await;
}
