//! Webhook events -> [`Inbound`]
//! (<https://developers.line.biz/en/reference/messaging-api/#webhook-event-objects>).
//! The body was already verified ([`crate::signature`]).
//!
//! | LINE event | Inbound |
//! |---|---|
//! | `message` text | `Text` |
//! | `message` image / video / audio / file | `File` (content id, or the external URL) |
//! | `postback` | `Action` (`data` = ActionId) |
//! | `follow` | `Follow`, or `Unblocked` when `follow.isUnblocked` is true |
//! | `unfollow` | `Blocked` |
//!
//! LINE has no separate unblock event: an unblock arrives as a `follow`
//! with `isUnblocked: true`
//! (<https://developers.line.biz/en/reference/messaging-api/#follow-event>),
//! which becomes `Unblocked`; the Kernel treats any Inbound from an
//! unreachable account as active again anyway. Skipped (no Inbound):
//! events in `standby` mode, from groups or rooms, and every other type
//! (sticker, location, join, beacon, unsend, ...).

use harpseal::{
    ActionId, ChannelAccountId, Error, Inbound, IncomingFile, ReplyToken, Result, Timestamp,
};
use serde_json::Value;

use crate::KIND;

/// How long a reply token is taken to be usable after the event's
/// `timestamp`: LINE says to reply within one minute
/// (<https://developers.line.biz/en/reference/messaging-api/#send-reply-message>).
/// A spent or late token is caught at send time and becomes a Push.
pub const REPLY_TOKEN_TTL_MS: i64 = 60_000;

/// Prefix of an [`IncomingFile::platform_file_id`] that is a URL: content
/// from an external provider is not on LINE's content API.
pub(crate) const EXTERNAL: &str = "https://";

/// Parses a verified body. A body that is not a webhook (no `events`
/// array) or an event LINE should never send (a postback whose `data` is
/// not a valid ActionId) is `Error::Invalid`.
pub(crate) fn parse(body: &[u8]) -> Result<Vec<Inbound>> {
    let v: Value = serde_json::from_slice(body)
        .map_err(|e| Error::Invalid(format!("LINE webhook body: {e}")))?;
    let events = v["events"]
        .as_array()
        .ok_or_else(|| Error::Invalid("LINE webhook body without `events`".into()))?;
    let mut out = Vec::with_capacity(events.len());
    for ev in events {
        if let Some(i) = event(ev)? {
            out.push(i);
        }
    }
    Ok(out)
}

fn event(ev: &Value) -> Result<Option<Inbound>> {
    if !ev.is_object() {
        return Err(Error::Invalid("LINE webhook event is not an object".into()));
    }
    let user = ev["source"]["userId"].as_str().filter(|u| !u.is_empty());
    let (Some(user), "user") = (user, ev["source"]["type"].as_str().unwrap_or("")) else {
        return Ok(None);
    };
    if ev["mode"].as_str() == Some("standby") {
        return Ok(None);
    }
    let from = ChannelAccountId::new(KIND, user);
    let reply = ev["replyToken"]
        .as_str()
        .filter(|t| !t.is_empty())
        .map(|t| ReplyToken {
            token: t.to_string(),
            expires_at: Timestamp(ev["timestamp"].as_i64().unwrap_or(0))
                .plus_ms(REPLY_TOKEN_TTL_MS),
        });
    Ok(match ev["type"].as_str().unwrap_or("") {
        "message" => message(from, &ev["message"], reply),
        "postback" => Some(Inbound::Action {
            from,
            action: postback(&ev["postback"])?,
            reply,
        }),
        "follow" if ev["follow"]["isUnblocked"].as_bool() == Some(true) => {
            Some(Inbound::Unblocked { from, reply })
        }
        "follow" => Some(Inbound::Follow { from, reply }),
        "unfollow" => Some(Inbound::Blocked { from }),
        _ => None,
    })
}

fn postback(p: &Value) -> Result<ActionId> {
    let data = p["data"]
        .as_str()
        .ok_or_else(|| Error::Invalid("LINE postback without `data`".into()))?;
    ActionId::new(data)
}

fn message(from: ChannelAccountId, m: &Value, reply: Option<ReplyToken>) -> Option<Inbound> {
    let kind = m["type"].as_str()?;
    if kind == "text" {
        let text = m["text"].as_str()?.to_string();
        return Some(Inbound::Text { from, text, reply });
    }
    if !matches!(kind, "image" | "video" | "audio" | "file") {
        return None;
    }
    let provider = &m["contentProvider"];
    let id = match provider["originalContentUrl"].as_str() {
        Some(url) if provider["type"] == "external" && url.starts_with(EXTERNAL) => url,
        _ => m["id"].as_str().filter(|i| !i.is_empty())?,
    };
    let file = IncomingFile {
        platform_file_id: id.to_string(),
        file_name: m["fileName"].as_str().map(str::to_string),
        content_type: None,
        size_bytes: m["fileSize"].as_u64(),
    };
    Some(Inbound::File { from, file, reply })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn one(ev: Value) -> Result<Vec<Inbound>> {
        parse(
            json!({ "destination": "U0", "events": [ev] })
                .to_string()
                .as_bytes(),
        )
    }

    fn ev(kind: &str, extra: Value) -> Value {
        let mut v = json!({
            "type": kind, "mode": "active", "timestamp": 1_000,
            "source": { "type": "user", "userId": "U1" }, "replyToken": "rt",
        });
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    }

    #[test]
    fn reply_token_expires_a_minute_after_the_event() {
        let got = one(ev("follow", json!({}))).unwrap();
        assert_eq!(got[0].reply().unwrap().expires_at, Timestamp(61_000));
    }

    #[test]
    fn postback_data_must_be_an_action_id() {
        let long = ev(
            "postback",
            json!({ "postback": { "data": "a".repeat(65) } }),
        );
        assert!(matches!(one(long), Err(Error::Invalid(_))));
        let missing = ev("postback", json!({ "postback": {} }));
        assert!(matches!(one(missing), Err(Error::Invalid(_))));
        let number = ev("postback", json!({ "postback": { "data": 7 } }));
        assert!(matches!(one(number), Err(Error::Invalid(_))));
    }

    #[test]
    fn media_messages_are_files() {
        for kind in ["image", "video", "audio"] {
            let got = one(ev(
                "message",
                json!({ "message": { "id": "42", "type": kind } }),
            ));
            match got.unwrap().as_slice() {
                [Inbound::File { file, .. }] => assert_eq!(file.platform_file_id, "42"),
                other => panic!("{kind}: {other:?}"),
            }
        }
        let external = json!({ "message": { "id": "43", "type": "image", "contentProvider":
            { "type": "external", "originalContentUrl": "https://img.sealbay.example/a.jpg" } } });
        match one(ev("message", external)).unwrap().as_slice() {
            [Inbound::File { file, .. }] => {
                assert_eq!(file.platform_file_id, "https://img.sealbay.example/a.jpg")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn other_events_are_skipped() {
        let sticker = ev(
            "message",
            json!({ "message": { "id": "1", "type": "sticker" } }),
        );
        assert!(one(sticker).unwrap().is_empty());
        assert!(one(ev("join", json!({}))).unwrap().is_empty());
        let standby = ev("follow", json!({ "mode": "standby" }));
        assert!(one(standby).unwrap().is_empty());
        let group = ev(
            "follow",
            json!({ "source": { "type": "group", "groupId": "C1" } }),
        );
        assert!(one(group).unwrap().is_empty());
        assert!(matches!(parse(b"{}"), Err(Error::Invalid(_))));
        assert!(matches!(parse(b"[1]"), Err(Error::Invalid(_))));
    }
}
