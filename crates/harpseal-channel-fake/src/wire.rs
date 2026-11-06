//! The Fake Channel's webhook wire format: a JSON array of events, signed
//! with the hex SHA-256 of `secret`, a newline and the raw body. Not HMAC
//! and not for production: it only has to make a body-bound signature
//! that tests can produce and the adapter can check before parsing.

use harpseal::{
    ActionId, ChannelAccountId, ChannelKind, Error, Inbound, IncomingFile, ReplyToken, Result,
    Timestamp,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

/// The signature of `body` under `secret`.
pub(crate) fn sign(secret: &str, body: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(secret.as_bytes());
    h.update(b"\n");
    h.update(body);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares in time independent of where the first difference is.
pub(crate) fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Serialises events in the wire format.
pub(crate) fn encode(events: &[Inbound]) -> Vec<u8> {
    let list: Vec<Value> = events.iter().map(event).collect();
    serde_json::to_vec(&list).unwrap_or_default()
}

fn event(ev: &Inbound) -> Value {
    let mut o = Map::new();
    o.insert("type".into(), json!(ev.kind()));
    o.insert("from".into(), json!(ev.from().external_id));
    if let Some(r) = ev.reply() {
        o.insert(
            "reply".into(),
            json!({ "token": r.token, "expires_at": r.expires_at.ms() }),
        );
    }
    match ev {
        Inbound::Text { text, .. } => {
            o.insert("text".into(), json!(text));
        }
        Inbound::File { file, .. } => {
            o.insert(
                "file".into(),
                json!({
                    "id": file.platform_file_id,
                    "name": file.file_name,
                    "content_type": file.content_type,
                    "size": file.size_bytes,
                }),
            );
        }
        Inbound::Action { action, .. } => {
            o.insert("data".into(), json!(action.as_str()));
        }
        _ => {}
    }
    Value::Object(o)
}

/// Parses a verified body. `reply_tokens: false` drops every reply token,
/// as a Channel without the concept would never issue one.
pub(crate) fn decode(kind: ChannelKind, body: &[u8], reply_tokens: bool) -> Result<Vec<Inbound>> {
    let list: Vec<Value> = serde_json::from_slice(body)
        .map_err(|e| Error::Invalid(format!("fake webhook body: {e}")))?;
    list.iter()
        .map(|v| parse_event(kind, v, reply_tokens))
        .collect()
}

fn text_field(v: &Value, key: &str) -> Result<String> {
    v[key]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::Invalid(format!("fake event without `{key}`")))
}

fn parse_event(kind: ChannelKind, v: &Value, reply_tokens: bool) -> Result<Inbound> {
    let from = ChannelAccountId::new(kind, text_field(v, "from")?);
    let reply = match &v["reply"] {
        Value::Object(_) if reply_tokens => Some(ReplyToken {
            token: text_field(&v["reply"], "token")?,
            expires_at: Timestamp(v["reply"]["expires_at"].as_i64().unwrap_or(0)),
        }),
        _ => None,
    };
    Ok(match text_field(v, "type")?.as_str() {
        "follow" => Inbound::Follow { from, reply },
        "text" => Inbound::Text {
            from,
            text: text_field(v, "text")?,
            reply,
        },
        "file" => {
            let f = &v["file"];
            Inbound::File {
                from,
                file: IncomingFile {
                    platform_file_id: text_field(f, "id")?,
                    file_name: f["name"].as_str().map(str::to_string),
                    content_type: f["content_type"].as_str().map(str::to_string),
                    size_bytes: f["size"].as_u64(),
                },
                reply,
            }
        }
        "action" => Inbound::Action {
            from,
            action: ActionId::new(text_field(v, "data")?)?,
            reply,
        },
        "blocked" => Inbound::Blocked { from },
        "unblocked" => Inbound::Unblocked { from, reply },
        other => return Err(Error::Invalid(format!("fake event type `{other}`"))),
    })
}
