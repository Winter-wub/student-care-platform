//! Pure LINE renderer (issue #3): ADR-0003 model -> Messaging API JSON.
//! No I/O. Every limit below is cited; degrade rules are in
//! `docs/prototypes/message-render.md`.

use harpseal::message::{Attachment, Button, Card, Choices, Menu, Message};
use harpseal::render::{
    MENU_MORE_ACTION, RenderCx, char_len, file_link, mini_app_unavailable, split_text, truncate,
};
use serde_json::{Value, json};

// https://developers.line.biz/en/reference/messaging-api/#text-message
pub const TEXT_MAX: usize = 5000;
// https://developers.line.biz/en/reference/messaging-api/#flex-message (altText)
pub const ALT_TEXT_MAX: usize = 1500;
// https://developers.line.biz/en/reference/messaging-api/#f-carousel
pub const CAROUSEL_MAX: usize = 12;
pub const CAROUSEL_JSON_MAX: usize = 50_000;
// https://developers.line.biz/en/reference/messaging-api/#bubble
pub const BUBBLE_JSON_MAX: usize = 30_000;
// https://developers.line.biz/en/reference/messaging-api/#items-object
pub const QUICK_REPLY_MAX: usize = 13;
// https://developers.line.biz/en/reference/messaging-api/#action-object-label-spec
pub const QUICK_REPLY_LABEL_MAX: usize = 20;
pub const FLEX_BUTTON_LABEL_MAX: usize = 40;
pub const RICH_MENU_LABEL_MAX: usize = 20;
// https://developers.line.biz/en/reference/messaging-api/#postback-action
pub const POSTBACK_DATA_MAX: usize = 300;
pub const DISPLAY_TEXT_MAX: usize = 300;
// https://developers.line.biz/en/reference/messaging-api/#send-reply-message (messages: Max 5)
pub const MESSAGES_PER_REQUEST: usize = 5;
// https://developers.line.biz/en/reference/messaging-api/#image-message
pub const IMAGE_MAX_BYTES: u64 = 10 * 1_048_576;
pub const PREVIEW_MAX_BYTES: u64 = 1_048_576;
// https://developers.line.biz/en/reference/messaging-api/#rich-menu-object
pub const RICH_MENU_AREAS_MAX: usize = 20;
pub const CHAT_BAR_TEXT_MAX: usize = 14;
/// Ours, not LINE's: a Card body longer than this is sent as Text first, so
/// one bubble stays far below `BUBBLE_JSON_MAX` (Thai is 3 bytes per char).
pub const FLEX_BODY_SOFT_MAX: usize = 2000;

// The invariant that makes "postback data > 300" impossible.
const _: () = assert!(harpseal::message::ActionId::MAX_BYTES <= POSTBACK_DATA_MAX);

/// One `Message` -> LINE message objects (several when degraded). The
/// caller packs them into requests of at most `MESSAGES_PER_REQUEST`.
pub fn message(msg: &Message, cx: &RenderCx) -> Vec<Value> {
    match msg {
        Message::Text(s) => texts(s),
        Message::Card(c) => card(c, cx),
        Message::CardList(list) => carousels(&list.0, cx),
        Message::Choices(c) => choices(c, cx),
        Message::Attachment(a) => vec![attachment(a, cx)],
        _ => Vec::new(), // #[non_exhaustive]: unknown variants are a compile-time TODO
    }
}

fn texts(s: &str) -> Vec<Value> {
    split_text(s, TEXT_MAX)
        .into_iter()
        .map(|t| json!({ "type": "text", "text": t }))
        .collect()
}

fn flex(alt: &str, contents: Value) -> Value {
    json!({ "type": "flex", "altText": truncate(alt, ALT_TEXT_MAX), "contents": contents })
}

fn card(c: &Card, cx: &RenderCx) -> Vec<Value> {
    if char_len(&c.body) <= FLEX_BODY_SOFT_MAX {
        return vec![flex(&c.title, bubble(&c.title, &c.body, &c.buttons, cx))];
    }
    // Long body: the full text goes first as Text, the bubble keeps title + buttons.
    let mut out = texts(&format!("{}\n\n{}", c.title, c.body));
    if !c.buttons.is_empty() {
        out.push(flex(&c.title, bubble(&c.title, "", &c.buttons, cx)));
    }
    out
}

fn bubble(title: &str, body: &str, buttons: &[Button], cx: &RenderCx) -> Value {
    let mut contents = Vec::new();
    if !title.is_empty() {
        contents.push(
            json!({ "type": "text", "text": title, "weight": "bold", "size": "lg", "wrap": true }),
        );
    }
    if !body.is_empty() {
        contents.push(json!({ "type": "text", "text": body, "size": "sm", "wrap": true }));
    }
    let mut footer = Vec::new();
    for (i, b) in buttons.iter().enumerate() {
        let style = if i == 0 { "primary" } else { "secondary" };
        match flex_action(b, cx) {
            Some(action) => footer.push(
                json!({ "type": "button", "style": style, "height": "sm", "action": action }),
            ),
            None => footer.push(notice(b, cx)),
        }
    }
    let mut v = json!({
        "type": "bubble",
        "body": { "type": "box", "layout": "vertical", "spacing": "md", "contents": contents },
    });
    if !footer.is_empty() {
        v["footer"] =
            json!({ "type": "box", "layout": "vertical", "spacing": "sm", "contents": footer });
    }
    v
}

/// A visible line instead of a Mini App button nobody can open.
fn notice(b: &Button, cx: &RenderCx) -> Value {
    let label = match b {
        Button::Action { label, .. } | Button::MiniApp { label, .. } => label,
    };
    json!({ "type": "text", "text": mini_app_unavailable(cx, label), "size": "xs", "wrap": true, "color": "#888888" })
}

/// Postback for Actions, URI to the Mini App; `None` when no Mini App URL.
fn action_object(b: &Button, cx: &RenderCx, label_max: usize) -> Option<Value> {
    match b {
        Button::Action { label, action } => Some(json!({
            "type": "postback",
            "label": truncate(label, label_max),
            "data": action.as_str(),
            "displayText": truncate(label, DISPLAY_TEXT_MAX),
        })),
        Button::MiniApp { label, path } => cx
            .mini_app_url(path)
            .map(|uri| json!({ "type": "uri", "label": truncate(label, label_max), "uri": uri })),
    }
}

fn flex_action(b: &Button, cx: &RenderCx) -> Option<Value> {
    action_object(b, cx, FLEX_BUTTON_LABEL_MAX)
}

fn carousels(cards: &[Card], cx: &RenderCx) -> Vec<Value> {
    let bubbles: Vec<Value> = cards
        .iter()
        .map(|c| {
            bubble(
                &c.title,
                &truncate(&c.body, FLEX_BODY_SOFT_MAX),
                &c.buttons,
                cx,
            )
        })
        .collect();
    // Greedy split by count and by JSON size; order is kept.
    let mut chunks: Vec<(Vec<Value>, Vec<&str>)> = Vec::new();
    let mut size = 0;
    for (b, c) in bubbles.into_iter().zip(cards) {
        let len = b.to_string().len();
        let full = chunks
            .last()
            .is_none_or(|(v, _)| v.len() == CAROUSEL_MAX || size + len + 64 > CAROUSEL_JSON_MAX);
        if full {
            chunks.push((Vec::new(), Vec::new()));
            size = 0;
        }
        let (v, titles) = chunks.last_mut().expect("pushed above");
        v.push(b);
        titles.push(&c.title);
        size += len + 1;
    }
    let n = chunks.len();
    chunks
        .into_iter()
        .enumerate()
        .map(|(i, (contents, titles))| {
            let part = if n > 1 {
                format!(" ({}/{n})", i + 1)
            } else {
                String::new()
            };
            let items = if cx.is_thai() {
                "รายการ"
            } else {
                "items"
            };
            let alt = format!("{} {items}{part}: {}", titles.len(), titles.join(", "));
            flex(&alt, json!({ "type": "carousel", "contents": contents }))
        })
        .collect()
}

fn choices(c: &Choices, cx: &RenderCx) -> Vec<Value> {
    let fits = c.options.len() <= QUICK_REPLY_MAX
        && c.options.iter().all(|b| match b {
            Button::Action { label, .. } | Button::MiniApp { label, .. } => {
                char_len(label) <= QUICK_REPLY_LABEL_MAX
            }
        });
    if !fits {
        // Too many or too long for quick reply: a bubble shows every option.
        let as_card = Card {
            title: String::new(),
            body: c.text.clone(),
            buttons: c.options.clone(),
        };
        let mut out = card(&as_card, cx);
        if let Some(last) = out.last_mut().filter(|v| v["type"] == "flex") {
            last["altText"] = json!(truncate(&c.text, ALT_TEXT_MAX));
        }
        return out;
    }
    let mut text = c.text.clone();
    let mut items = Vec::new();
    for b in &c.options {
        match action_object(b, cx, QUICK_REPLY_LABEL_MAX) {
            Some(action) => items.push(json!({ "type": "action", "action": action })),
            None => text.push_str(&format!("\n{}", mini_app_unavailable(cx, label_of(b)))),
        }
    }
    let mut out = texts(&text);
    if let Some(last) = out.last_mut().filter(|_| !items.is_empty()) {
        last["quickReply"] = json!({ "items": items });
    }
    out
}

fn label_of(b: &Button) -> &str {
    match b {
        Button::Action { label, .. } | Button::MiniApp { label, .. } => label,
    }
}

fn attachment(a: &Attachment, cx: &RenderCx) -> Value {
    let is_image = a.content_type == "image/jpeg" || a.content_type == "image/png";
    // Same URL as preview, so the file itself must fit the 1 MB preview cap.
    if is_image && a.size_bytes <= PREVIEW_MAX_BYTES.min(IMAGE_MAX_BYTES) {
        return json!({ "type": "image", "originalContentUrl": a.url, "previewImageUrl": a.url });
    }
    // LINE has no file message for bots; video/audio need a preview/duration
    // the model does not carry. Everything else becomes a link.
    json!({ "type": "text", "text": file_link(cx, &a.file_name, a.size_bytes, &a.url) })
}

/// Rich menu object for `POST /v2/bot/richmenu`. A 3- or 4-column grid of
/// postback areas on a 2500 px wide image. The Core must still draw and
/// upload the image (`POST api-data.line.me/v2/bot/richmenu/{id}/content`,
/// JPEG/PNG, ≤ 1 MB, same size as `size`) and then link it to the user.
pub fn menu(m: &Menu) -> Value {
    let mut entries: Vec<(String, String)> = m
        .entries
        .iter()
        .map(|e| (e.label.clone(), e.action.as_str().to_string()))
        .collect();
    if entries.len() > RICH_MENU_AREAS_MAX {
        entries.truncate(RICH_MENU_AREAS_MAX - 1);
        entries.push(("เพิ่มเติม".into(), MENU_MORE_ACTION.into()));
    }
    let n = entries.len().max(1);
    let cols = match n {
        0..=3 => n,
        4 => 2,
        5 | 6 => 3,
        _ => 4,
    };
    let rows = n.div_ceil(cols);
    let (width, height) = (2500, if rows == 1 { 843 } else { 1686 });
    let (cw, ch) = (width / cols, height / rows);
    let areas: Vec<Value> = entries
        .iter()
        .enumerate()
        .map(|(i, (label, data))| {
            let (x, y) = ((i % cols) * cw, (i / cols) * ch);
            json!({
                "bounds": { "x": x, "y": y, "width": cw, "height": ch },
                "action": {
                    "type": "postback",
                    "label": truncate(label, RICH_MENU_LABEL_MAX),
                    "data": data,
                    "displayText": truncate(label, DISPLAY_TEXT_MAX),
                },
            })
        })
        .collect();
    json!({
        "size": { "width": width, "height": height },
        "selected": false,
        "name": format!("harpseal-{}", entries.len()),
        "chatBarText": truncate("เมนู", CHAT_BAR_TEXT_MAX),
        "areas": areas,
    })
}
