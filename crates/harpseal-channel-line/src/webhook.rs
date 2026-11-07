//! Webhook events -> [`Inbound`]
//! (<https://developers.line.biz/en/reference/messaging-api/#webhook-event-objects>).
//! The body was already verified ([`crate::signature`]).
//!
//! | LINE event | Inbound |
//! |---|---|
//! | `message` text | `Text` |
//! | `message` image / video / audio / file | `File` (content id; `external:<id>` for content LINE does not host) |
//! | `postback` | `Action` (`data` = ActionId) |
//! | `follow` | `Follow`, or `Unblocked` when `follow.isUnblocked` is true |
//! | `unfollow` | `Blocked` |
//!
//! LINE has no separate unblock event: an unblock arrives as a `follow`
//! with `isUnblocked: true`
//! (<https://developers.line.biz/en/reference/messaging-api/#follow-event>),
//! which becomes `Unblocked`; the Kernel treats any Inbound from an
//! unreachable account as active again anyway. Skipped (no Inbound):
//! events in `standby` mode, from groups or rooms, every other type
//! (sticker, location, join, beacon, unsend, ...), and events LINE should
//! never send (not an object, a postback whose `data` is not a valid
//! ActionId): one bad event never drops the others in the same call, and
//! never makes LINE redeliver the whole body.
//!
//! Every event carries LINE's `webhookEventId` as its event id and
//! `deliveryContext.isRedelivery` as its redelivery flag
//! (<https://developers.line.biz/en/reference/messaging-api/#common-properties>),
//! so the Kernel answers a redelivered event once.

use harpseal::{
    ActionId, ChannelAccountId, Error, Inbound, IncomingFile, ReplyToken, Result, Timestamp,
};
use serde_json::Value;

use crate::KIND;

/// How long a reply token is taken to be usable after the event's
/// `timestamp`: LINE says to reply within one minute
/// (<https://developers.line.biz/en/reference/messaging-api/#send-reply-message>).
/// A spent or late token is caught at send time and becomes a Push. An
/// event without a `timestamp` (LINE always sends one) gets a token with
/// no known expiry: the Core replies, and if LINE refuses the token the
/// adapter sends the same objects by Push.
pub const REPLY_TOKEN_TTL_MS: i64 = 60_000;

/// Prefix of an [`IncomingFile::platform_file_id`] whose content is held
/// by an external provider (`contentProvider.type = external`), not on
/// LINE's content API: `download` refuses it rather than fetch a URL the
/// sender chose.
pub(crate) const EXTERNAL: &str = "external:";

/// Parses a verified body. Only a body that is not a webhook (not JSON, no
/// `events` array) is `Error::Invalid`; bad events are skipped.
pub(crate) fn parse(body: &[u8]) -> Result<Vec<Inbound>> {
    let v: Value = serde_json::from_slice(body)
        .map_err(|e| Error::Invalid(format!("LINE webhook body: {e}")))?;
    let events = v["events"]
        .as_array()
        .ok_or_else(|| Error::Invalid("LINE webhook body without `events`".into()))?;
    let mut out = Vec::with_capacity(events.len());
    out.extend(events.iter().filter_map(event));
    Ok(out)
}

fn event(ev: &Value) -> Option<Inbound> {
    if !ev.is_object() {
        return None;
    }
    let user = ev["source"]["userId"].as_str().filter(|u| !u.is_empty());
    let (Some(user), "user") = (user, ev["source"]["type"].as_str().unwrap_or("")) else {
        return None;
    };
    if ev["mode"].as_str() == Some("standby") {
        return None;
    }
    let from = ChannelAccountId::new(KIND, user);
    let reply = ev["replyToken"]
        .as_str()
        .filter(|t| !t.is_empty())
        .map(|t| ReplyToken {
            token: t.to_string(),
            expires_at: ev["timestamp"].as_i64().map_or(Timestamp(i64::MAX), |t| {
                Timestamp(t).plus_ms(REPLY_TOKEN_TTL_MS)
            }),
        });
    let event_id = ev["webhookEventId"]
        .as_str()
        .filter(|i| !i.is_empty())
        .map(str::to_string);
    let redelivery = ev["deliveryContext"]["isRedelivery"].as_bool() == Some(true);
    let inbound = match ev["type"].as_str().unwrap_or("") {
        "message" => message(from, &ev["message"], reply),
        "postback" => Some(Inbound::action(from, postback(&ev["postback"])?, reply)),
        "follow" if ev["follow"]["isUnblocked"].as_bool() == Some(true) => {
            Some(Inbound::unblocked(from, reply))
        }
        "follow" => Some(Inbound::follow(from, reply)),
        "unfollow" => Some(Inbound::blocked(from)),
        _ => None,
    };
    inbound.map(|e| e.with_event_id(event_id, redelivery))
}

/// `data` as an ActionId; `None` when missing or not 1..=64 bytes.
fn postback(p: &Value) -> Option<ActionId> {
    ActionId::new(p["data"].as_str()?).ok()
}

fn message(from: ChannelAccountId, m: &Value, reply: Option<ReplyToken>) -> Option<Inbound> {
    let kind = m["type"].as_str()?;
    if kind == "text" {
        let text = m["text"].as_str()?.to_string();
        return Some(Inbound::text(from, text, reply));
    }
    if !matches!(kind, "image" | "video" | "audio" | "file") {
        return None;
    }
    let id = m["id"].as_str().filter(|i| !i.is_empty())?;
    let platform_file_id = if m["contentProvider"]["type"] == "external" {
        format!("{EXTERNAL}{id}")
    } else {
        id.to_string()
    };
    let file = IncomingFile {
        platform_file_id,
        file_name: m["fileName"].as_str().map(str::to_string),
        content_type: None,
        size_bytes: m["fileSize"].as_u64(),
    };
    Some(Inbound::file(from, file, reply))
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
        let mut no_time = ev("follow", json!({}));
        no_time.as_object_mut().unwrap().remove("timestamp");
        let got = one(no_time).unwrap();
        assert_eq!(
            got[0].reply().unwrap().expires_at,
            Timestamp(i64::MAX),
            "never an epoch expiry that silently turns the Reply into a Push"
        );
    }

    #[test]
    fn a_bad_postback_is_skipped_and_the_others_kept() {
        let bad = [
            json!({ "postback": { "data": "a".repeat(65) } }),
            json!({ "postback": {} }),
            json!({ "postback": { "data": 7 } }),
        ];
        for b in bad {
            let body = json!({ "events": [
                ev("postback", b),
                ev("postback", json!({ "postback": { "data": "news.ack.1" } })),
                "not an event",
            ]});
            let got = parse(body.to_string().as_bytes()).unwrap();
            match got.as_slice() {
                [Inbound::Action { action, .. }] => assert_eq!(action.as_str(), "news.ack.1"),
                other => panic!("{other:?}"),
            }
        }
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
            [Inbound::File { file, .. }] => assert_eq!(file.platform_file_id, "external:43"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn event_ids_and_redelivery_come_from_the_event() {
        let got = one(ev(
            "follow",
            json!({ "webhookEventId": "01HSEALBAY0000000000000001",
                    "deliveryContext": { "isRedelivery": true } }),
        ))
        .unwrap();
        assert_eq!(got[0].event_id(), Some("01HSEALBAY0000000000000001"));
        assert!(got[0].redelivery());
        let got = one(ev(
            "unfollow",
            json!({ "webhookEventId": "01HSEALBAY0000000000000002",
                    "deliveryContext": { "isRedelivery": false } }),
        ))
        .unwrap();
        assert_eq!(got[0].event_id(), Some("01HSEALBAY0000000000000002"));
        assert!(!got[0].redelivery());
        let got = one(ev("follow", json!({}))).unwrap();
        assert_eq!((got[0].event_id(), got[0].redelivery()), (None, false));
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
