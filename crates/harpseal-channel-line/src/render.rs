//! Pure LINE renderer (issue #3): ADR-0003 model -> Messaging API JSON.
//! No I/O. Every limit below is cited; degrade rules are in
//! `docs/prototypes/message-render.md`.

use harpseal::render::{
    MENU_MORE_ACTION, RenderCx, char_len, file_link, label_or_host, link_unavailable,
    mini_app_unavailable, split_text, truncate,
};
use harpseal::{Attachment, Button, Card, CardImage, Choices, Menu, Message};
use serde_json::{Value, json};

/// <https://developers.line.biz/en/reference/messaging-api/#text-message>
pub const TEXT_MAX: usize = 5000;
/// <https://developers.line.biz/en/reference/messaging-api/#flex-message> (altText)
pub const ALT_TEXT_MAX: usize = 1500;
/// <https://developers.line.biz/en/reference/messaging-api/#f-carousel>
pub const CAROUSEL_MAX: usize = 12;
/// `CAROUSEL_JSON_MAX`: see the platform reference cited above.
pub const CAROUSEL_JSON_MAX: usize = 50_000;
/// <https://developers.line.biz/en/reference/messaging-api/#bubble>
pub const BUBBLE_JSON_MAX: usize = 30_000;
/// <https://developers.line.biz/en/reference/messaging-api/#items-object>
pub const QUICK_REPLY_MAX: usize = 13;
/// <https://developers.line.biz/en/reference/messaging-api/#action-object-label-spec>
pub const QUICK_REPLY_LABEL_MAX: usize = 20;
/// `FLEX_BUTTON_LABEL_MAX`: see the platform reference cited above.
pub const FLEX_BUTTON_LABEL_MAX: usize = 40;
/// `RICH_MENU_LABEL_MAX`: see the platform reference cited above.
pub const RICH_MENU_LABEL_MAX: usize = 20;
/// <https://developers.line.biz/en/reference/messaging-api/#postback-action>
pub const POSTBACK_DATA_MAX: usize = 300;
/// `DISPLAY_TEXT_MAX`: see the platform reference cited above.
pub const DISPLAY_TEXT_MAX: usize = 300;
/// <https://developers.line.biz/en/reference/messaging-api/#send-reply-message> (messages: Max 5)
pub const MESSAGES_PER_REQUEST: usize = 5;
/// <https://developers.line.biz/en/reference/messaging-api/#image-message>
pub const IMAGE_MAX_BYTES: u64 = 10 * 1_048_576;
/// `PREVIEW_MAX_BYTES`: see the platform reference cited above.
pub const PREVIEW_MAX_BYTES: u64 = 1_048_576;
/// <https://developers.line.biz/en/reference/messaging-api/#rich-menu-object>
pub const RICH_MENU_AREAS_MAX: usize = 20;
/// `CHAT_BAR_TEXT_MAX`: see the platform reference cited above.
pub const CHAT_BAR_TEXT_MAX: usize = 14;
/// Ours, not LINE's: a Card body longer than this is sent as Text first, so
/// one bubble stays far below `BUBBLE_JSON_MAX` (Thai is 3 bytes per char).
pub const FLEX_BODY_SOFT_MAX: usize = 2000;
/// The hero image's aspect ratio (#109): LINE's default "20:13" with
/// `aspectMode: cover`, so every announcement Card has the same shape
/// (<https://developers.line.biz/en/reference/messaging-api/#f-image>).
pub const HERO_ASPECT_RATIO: &str = "20:13";

// The invariant that makes "postback data > 300" impossible.
const _: () = assert!(harpseal::ActionId::MAX_BYTES <= POSTBACK_DATA_MAX);

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
    let image = c.image.as_ref();
    if char_len(&c.body) <= FLEX_BODY_SOFT_MAX {
        return vec![flex(
            &c.title,
            bubble(&c.title, &c.body, &c.buttons, image, cx),
        )];
    }
    // Long body: the full text goes first as Text, the bubble keeps title + buttons.
    let mut out = texts(&format!("{}\n\n{}", c.title, c.body));
    if !c.buttons.is_empty() || image.is_some() {
        out.push(flex(&c.title, bubble(&c.title, "", &c.buttons, image, cx)));
    }
    out
}

/// A Card image as the bubble's hero (#109): full width, 20:13, cover.
/// A URL [`CardImage::safe_url`] refuses leaves the bubble without one.
fn hero(image: Option<&CardImage>) -> Option<Value> {
    let url = image?.safe_url()?;
    Some(json!({
        "type": "image", "url": url, "size": "full",
        "aspectRatio": HERO_ASPECT_RATIO, "aspectMode": "cover",
    }))
}

fn bubble(
    title: &str,
    body: &str,
    buttons: &[Button],
    image: Option<&CardImage>,
    cx: &RenderCx,
) -> Value {
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
    if let Some(h) = hero(image) {
        v["hero"] = h;
    }
    if !footer.is_empty() {
        v["footer"] =
            json!({ "type": "box", "layout": "vertical", "spacing": "sm", "contents": footer });
    }
    v
}

/// A visible line instead of a Mini App button nobody can open, or a URL
/// button refused.
fn notice(b: &Button, cx: &RenderCx) -> Value {
    json!({ "type": "text", "text": unavailable(b, cx), "size": "xs", "wrap": true, "color": "#888888" })
}

/// The notice text for a button that cannot render.
fn unavailable(b: &Button, cx: &RenderCx) -> String {
    match b {
        Button::Url { .. } => link_unavailable(cx, b.label()),
        _ => mini_app_unavailable(cx, b.label()),
    }
}

/// Postback for Actions, URI to the Mini App or to a URL button's page;
/// `None` when no Mini App URL or a refused URL.
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
        // A URI action outside LINE; a URL that is not https is refused
        // and shows as the visible notice instead.
        Button::Url { label, .. } => b.safe_url().map(|uri| {
            let label = truncate(&label_or_host(label, uri), label_max);
            json!({ "type": "uri", "label": label, "uri": uri })
        }),
        // #[non_exhaustive]: an unknown kind renders as the visible notice.
        _ => None,
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
                c.image.as_ref(),
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
        && c.options
            .iter()
            .all(|b| char_len(b.label()) <= QUICK_REPLY_LABEL_MAX);
    if !fits {
        // Too many or too long for quick reply: a bubble shows every option.
        let as_card = Card {
            image: None,
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
            None => text.push_str(&format!("\n{}", unavailable(b, cx))),
        }
    }
    let mut out = texts(&text);
    if let Some(last) = out.last_mut().filter(|_| !items.is_empty()) {
        last["quickReply"] = json!({ "items": items });
    }
    out
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
/// postback areas on a 2500 px wide image, 843 px high for one row and
/// 1686 px for more. [`crate::LineChannel`]'s `set_menu` uses
/// [`menu_sized`] with the size of the configured image instead.
pub fn menu(m: &Menu) -> Value {
    let rows = grid(menu_entries(m).len()).1;
    menu_sized(m, 2500, if rows == 1 { 843 } else { 1686 })
}

/// Columns and rows for `n` areas.
fn grid(n: usize) -> (usize, usize) {
    let n = n.max(1);
    let cols = match n {
        0..=3 => n,
        4 => 2,
        5 | 6 => 3,
        _ => 4,
    };
    (cols, n.div_ceil(cols))
}

fn menu_entries(m: &Menu) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = m
        .entries
        .iter()
        .map(|e| (e.label.clone(), e.action.as_str().to_string()))
        .collect();
    if entries.len() > RICH_MENU_AREAS_MAX {
        entries.truncate(RICH_MENU_AREAS_MAX - 1);
        entries.push(("เพิ่มเติม".into(), MENU_MORE_ACTION.into()));
    }
    entries
}

/// [`menu`] on an image of `width` x `height` px: the same grid, scaled.
pub fn menu_sized(m: &Menu, width: usize, height: usize) -> Value {
    let entries = menu_entries(m);
    let (cols, rows) = grid(entries.len());
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
