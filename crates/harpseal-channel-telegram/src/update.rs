//! Webhook calls: the secret-token check and one `Update` → at most one
//! [`Inbound`] (<https://core.telegram.org/bots/api#update>).
//!
//! Telegram posts one Update per call. Updates the Core has no event for
//! (edited messages, stickers, group chats, …) give no event, so Telegram
//! is answered 200 and does not resend them.

use std::collections::HashMap;

use harpseal::{ActionId, ChannelAccountId, Error, Inbound, IncomingFile, Result, WebhookRequest};
use serde_json::Value;

use crate::KIND;

/// The header Telegram sends with the `secret_token` given to
/// `setWebhook` (<https://core.telegram.org/bots/api#setwebhook>).
pub const SECRET_HEADER: &str = "x-telegram-bot-api-secret-token";

/// Command name (without `/`) → the Action it triggers.
pub type CommandMap = HashMap<String, ActionId>;

/// Compares in time independent of where the bytes differ. The length
/// may leak, which is fine: the secret's length is not secret enough to
/// matter and Telegram allows 1-256 characters.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `Err(BadSignature)` unless the header equals `secret` (never empty).
pub fn verify(req: &WebhookRequest, secret: &str) -> Result<()> {
    let given = req
        .headers
        .get(SECRET_HEADER)
        .map(|v| v.as_bytes())
        .unwrap_or_default();
    if secret.is_empty() || !constant_time_eq(given, secret.as_bytes()) {
        return Err(Error::BadSignature);
    }
    Ok(())
}

/// What one Update asks of the adapter besides the event: the id of a
/// callback query to answer so the Student's client stops spinning, and
/// why the Update is refused after that answer.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Parsed {
    /// The event, when the Update is one the Core handles.
    pub event: Option<Inbound>,
    /// `callback_query.id`, to pass to `answerCallbackQuery`.
    pub answer_callback: Option<String>,
    /// A press with malformed `callback_data`: answer it, then refuse the
    /// call with this `Error::Invalid` (Telegram will redeliver it).
    pub refused: Option<Error>,
}

fn invalid(what: &str) -> Error {
    Error::Invalid(format!("Telegram update: {what}"))
}

fn account(chat_id: &Value) -> Result<ChannelAccountId> {
    chat_id
        .as_i64()
        .map(|id| ChannelAccountId::new(KIND, id.to_string()))
        .ok_or_else(|| invalid("no chat id"))
}

/// Parses an already verified body. `lookup` resolves a command name for
/// the sending account (the Menu overlay, then the construction-time map).
pub fn parse(
    body: &[u8],
    lookup: impl Fn(&ChannelAccountId, &str) -> Option<ActionId>,
) -> Result<Parsed> {
    let update: Value = serde_json::from_slice(body).map_err(|e| invalid(&e.to_string()))?;
    if !update["update_id"].is_i64() {
        return Err(invalid("no update_id"));
    }
    if let Some(m) = update.get("message") {
        return Ok(Parsed {
            event: message(m, lookup)?,
            ..Parsed::default()
        });
    }
    if let Some(q) = update.get("callback_query") {
        return callback(q);
    }
    if let Some(m) = update.get("my_chat_member") {
        return Ok(Parsed {
            event: member(m)?,
            ..Parsed::default()
        });
    }
    Ok(Parsed::default())
}

fn private(chat: &Value) -> bool {
    chat["type"] == "private"
}

fn message(
    m: &Value,
    lookup: impl Fn(&ChannelAccountId, &str) -> Option<ActionId>,
) -> Result<Option<Inbound>> {
    if !private(&m["chat"]) {
        return Ok(None);
    }
    let from = account(&m["chat"]["id"])?;
    if let Some(text) = m["text"].as_str() {
        return Ok(Some(text_event(from, text, lookup)));
    }
    Ok(incoming_file(m).map(|file| Inbound::File {
        from,
        file,
        reply: None,
    }))
}

/// `/start` (with or without a deep-link payload) is the Follow; a known
/// command is its Action; anything else is Text.
fn text_event(
    from: ChannelAccountId,
    text: &str,
    lookup: impl Fn(&ChannelAccountId, &str) -> Option<ActionId>,
) -> Inbound {
    if let Some(cmd) = command(text) {
        if cmd == "start" {
            return Inbound::Follow { from, reply: None };
        }
        if let Some(action) = lookup(&from, &cmd) {
            return Inbound::Action {
                from,
                action,
                reply: None,
            };
        }
    }
    Inbound::Text {
        from,
        text: text.to_string(),
        reply: None,
    }
}

/// `/news_latest`, `/news_latest@SealBayBot` or `/news_latest args` →
/// `news_latest` (<https://core.telegram.org/bots/features#commands>). The
/// name starts right after `/`: `/ news_latest` is not a command.
pub fn command(text: &str) -> Option<String> {
    let rest = text.strip_prefix('/')?;
    if rest.starts_with(char::is_whitespace) {
        return None;
    }
    let first = rest.split_whitespace().next()?;
    let name = first.split('@').next()?;
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}

/// The file in a message: the largest `photo` size by area, or a `document`,
/// `voice`, `video` or `audio`.
fn incoming_file(m: &Value) -> Option<IncomingFile> {
    if let Some(sizes) = m["photo"].as_array() {
        let area = |p: &Value| p["width"].as_u64().unwrap_or(0) * p["height"].as_u64().unwrap_or(0);
        let best = sizes.iter().max_by_key(|p| area(p))?;
        return Some(IncomingFile {
            platform_file_id: best["file_id"].as_str()?.to_string(),
            file_name: None,
            content_type: Some("image/jpeg".into()),
            size_bytes: best["file_size"].as_u64(),
        });
    }
    let f = ["document", "voice", "video", "audio"]
        .iter()
        .find_map(|k| m.get(*k))?;
    Some(IncomingFile {
        platform_file_id: f["file_id"].as_str()?.to_string(),
        file_name: f["file_name"].as_str().map(String::from),
        content_type: f["mime_type"].as_str().map(String::from),
        size_bytes: f["file_size"].as_u64(),
    })
}

/// A button press: `data` is the Action id, unchanged. Presses outside a
/// private chat (groups, inline messages) are answered and ignored.
fn callback(q: &Value) -> Result<Parsed> {
    let id = q["id"]
        .as_str()
        .ok_or_else(|| invalid("callback without id"))?;
    let mut parsed = Parsed {
        answer_callback: Some(id.to_string()),
        ..Parsed::default()
    };
    let chat = &q["message"]["chat"];
    if !private(chat) {
        return Ok(parsed);
    }
    let from = account(&chat["id"])?;
    match q["data"].as_str().map(ActionId::new) {
        Some(Ok(action)) => {
            parsed.event = Some(Inbound::Action {
                from,
                action,
                reply: None,
            })
        }
        _ => parsed.refused = Some(invalid("callback_data is not an Action id")),
    }
    Ok(parsed)
}

/// The Student blocked (`kicked`) or deleted (`left`) the chat, or came
/// back (`member`) (<https://core.telegram.org/bots/api#chatmember>).
fn member(m: &Value) -> Result<Option<Inbound>> {
    if !private(&m["chat"]) {
        return Ok(None);
    }
    let from = account(&m["chat"]["id"])?;
    Ok(match m["new_chat_member"]["status"].as_str() {
        Some("kicked" | "left") => Some(Inbound::Blocked { from }),
        Some("member") => Some(Inbound::Unblocked { from, reply: None }),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn none(_: &ChannelAccountId, _: &str) -> Option<ActionId> {
        None
    }

    fn msg(fields: Value) -> Vec<u8> {
        let mut m = json!({ "message_id": 1, "date": 1, "chat": { "id": 42, "type": "private" } });
        for (k, v) in fields.as_object().unwrap() {
            m[k] = v.clone();
        }
        json!({ "update_id": 1, "message": m })
            .to_string()
            .into_bytes()
    }

    #[test]
    fn commands_are_parsed() {
        assert_eq!(command("/news_latest").as_deref(), Some("news_latest"));
        assert_eq!(
            command("/News_Latest@SealBayBot now").as_deref(),
            Some("news_latest")
        );
        assert_eq!(command("news_latest"), None);
        assert_eq!(command("/"), None);
        assert_eq!(command("/ news_latest"), None);
    }

    #[test]
    fn files_of_every_kind() {
        let photo = msg(json!({ "photo": [
            { "file_id": "big", "width": 1280, "height": 960 },
            { "file_id": "small", "width": 90, "height": 67 } ] }));
        let voice =
            msg(json!({ "voice": { "file_id": "v1", "mime_type": "audio/ogg", "file_size": 5 } }));
        let video = msg(
            json!({ "video": { "file_id": "m1", "file_name": "lab.mp4", "mime_type": "video/mp4" } }),
        );
        for (body, id) in [(photo, "big"), (voice, "v1"), (video, "m1")] {
            match parse(&body, none).unwrap().event {
                Some(Inbound::File { file, .. }) => assert_eq!(file.platform_file_id, id),
                other => panic!("{id}: {other:?}"),
            }
        }
    }

    #[test]
    fn left_is_blocked_and_unknown_updates_are_ignored() {
        let left = json!({ "update_id": 1, "my_chat_member": {
            "chat": { "id": 42, "type": "private" }, "new_chat_member": { "status": "left" } } });
        let ev = parse(left.to_string().as_bytes(), none).unwrap().event;
        assert!(matches!(ev, Some(Inbound::Blocked { .. })));
        let edited = json!({ "update_id": 2, "edited_message": {} });
        assert!(
            parse(edited.to_string().as_bytes(), none)
                .unwrap()
                .event
                .is_none()
        );
        let group = msg(json!({ "text": "hi", "chat": { "id": -5, "type": "group" } }));
        assert!(parse(&group, none).unwrap().event.is_none());
        let sticker = msg(json!({ "sticker": { "file_id": "s" } }));
        assert!(parse(&sticker, none).unwrap().event.is_none());
    }

    #[test]
    fn malformed_callback_data_is_answered_then_refused() {
        let q = |data: Value, chat: &str| {
            json!({ "update_id": 1, "callback_query": { "id": "q", "from": { "id": 42 },
                "message": { "message_id": 1, "chat": { "id": 42, "type": chat } }, "data": data } })
            .to_string()
            .into_bytes()
        };
        for bad in [json!("x".repeat(65)), json!(""), Value::Null] {
            let p = parse(&q(bad, "private"), none).unwrap();
            assert_eq!(p.answer_callback.as_deref(), Some("q"));
            assert!(p.event.is_none());
            assert!(matches!(p.refused, Some(Error::Invalid(_))));
        }
        let ok = parse(&q(json!("news.ack.1042"), "private"), none).unwrap();
        assert_eq!(ok.answer_callback.as_deref(), Some("q"));
        assert!(ok.refused.is_none());
        assert!(matches!(ok.event, Some(Inbound::Action { from, .. }) if from.external_id == "42"));
        let group = parse(&q(json!("news.ack.1042"), "group"), none).unwrap();
        assert_eq!(group.answer_callback.as_deref(), Some("q"));
        assert!(group.event.is_none() && group.refused.is_none());
    }

    #[test]
    fn a_body_without_update_id_is_invalid() {
        assert!(matches!(parse(b"{}", none), Err(Error::Invalid(_))));
        assert!(matches!(parse(b"[1]", none), Err(Error::Invalid(_))));
    }

    #[test]
    fn secret_comparison() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        let req = WebhookRequest {
            headers: http::HeaderMap::new(),
            body: vec![],
        };
        assert!(matches!(verify(&req, ""), Err(Error::BadSignature)));
    }
}
