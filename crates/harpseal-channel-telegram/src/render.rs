//! Pure Telegram renderer (issue #3): ADR-0003 model -> Bot API JSON.
//! Each Value is one method call with a `method` field (the form the Bot API
//! accepts in a webhook answer: <https://core.telegram.org/bots/api#making-requests-when-getting-updates>).
//! `chat_id` is added by `send`. Degrade rules: `docs/prototypes/message-render.md`.

use harpseal::render::{
    MENU_MORE_ACTION, RenderCx, char_len, file_link, link_unavailable, mini_app_unavailable,
    more_label, split_text, strip_format_chars, truncate,
};
use harpseal::{ActionId, Attachment, Button, Card, Choices, Menu, Message};
use serde_json::{Value, json};

/// Ours, not Telegram's (the Bot API states no button text limit): a URL
/// button's label is cut like LINE's Flex button label.
pub const URL_LABEL_MAX: usize = 40;
/// <https://core.telegram.org/bots/api#sendmessage> (1-4096 chars after entity parsing)
pub const TEXT_MAX: usize = 4096;
/// <https://core.telegram.org/bots/api#inlinekeyboardbutton>
pub const CALLBACK_DATA_MAX: usize = 64;
/// UNCONFIRMED: the Bot API states no button count. Conservative ceiling
/// from docs/research/first-slice-facts.md §2.1; test on a real bot.
pub const KEYBOARD_BUTTONS_MAX: usize = 100;
/// <https://core.telegram.org/bots/api#setmycommands,> #botcommand
pub const COMMANDS_MAX: usize = 100;
/// `COMMAND_MAX`: see the platform reference cited above.
pub const COMMAND_MAX: usize = 32;
/// `COMMAND_DESCRIPTION_MAX`: see the platform reference cited above.
pub const COMMAND_DESCRIPTION_MAX: usize = 256;
/// <https://core.telegram.org/bots/api#sending-files>
pub const PHOTO_BY_URL_MAX: u64 = 5 * 1_048_576;
/// `FILE_BY_URL_MAX`: see the platform reference cited above.
pub const FILE_BY_URL_MAX: u64 = 20 * 1_048_576;
/// `PHOTO_UPLOAD_MAX`: see the platform reference cited above.
pub const PHOTO_UPLOAD_MAX: u64 = 10 * 1_048_576;
/// <https://core.telegram.org/bots/api#senddocument>
pub const FILE_UPLOAD_MAX: u64 = 50 * 1_048_576;
/// <https://core.telegram.org/bots/api#senddocument> (caption 0-1024)
pub const CAPTION_MAX: usize = 1024;

const _: () = assert!(ActionId::MAX_BYTES <= CALLBACK_DATA_MAX);

/// Escapes text for `parse_mode: HTML` (<https://core.telegram.org/bots/api#html-style>).
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// `message`: see the platform reference cited above.
pub fn message(msg: &Message, cx: &RenderCx) -> Vec<Value> {
    match msg {
        Message::Text(s) => split_text(s, TEXT_MAX)
            .into_iter()
            .map(|t| send_message(json!(t), false))
            .collect(),
        Message::Card(c) => card(c, cx),
        // No carousel on Telegram: one message per Card, in order.
        Message::CardList(list) => list.0.iter().flat_map(|c| card(c, cx)).collect(),
        Message::Choices(c) => choices(c, cx),
        Message::Attachment(a) => vec![attachment(a, cx)],
        _ => Vec::new(),
    }
}

fn send_message(text: Value, html: bool) -> Value {
    let mut v = json!({ "method": "sendMessage", "text": text });
    if html {
        v["parse_mode"] = json!("HTML");
    }
    v
}

/// Title bold + body. Lengths are measured on the plain text (what Telegram
/// counts after entity parsing), then each chunk is escaped.
fn card(c: &Card, cx: &RenderCx) -> Vec<Value> {
    let body = with_notices(&c.body, &c.buttons, cx);
    let budget = TEXT_MAX.saturating_sub(char_len(&c.title) + 1).max(1);
    let mut chunks = split_text(&body, budget);
    let first = chunks.remove(0);
    let mut out = vec![send_message(
        json!(format!(
            "<b>{}</b>\n{}",
            escape_html(&c.title),
            escape_html(&first)
        )),
        true,
    )];
    for rest in chunks {
        out.push(send_message(json!(escape_html(&rest)), true));
    }
    attach_keyboards(&mut out, &c.buttons, cx);
    out
}

fn choices(c: &Choices, cx: &RenderCx) -> Vec<Value> {
    let text = with_notices(&c.text, &c.options, cx);
    let mut out: Vec<Value> = split_text(&text, TEXT_MAX)
        .into_iter()
        .map(|t| send_message(json!(t), false))
        .collect();
    attach_keyboards(&mut out, &c.options, cx);
    out
}

/// Appends a visible line for each Mini App button that cannot render
/// and each URL button refused (not https).
fn with_notices(text: &str, buttons: &[Button], cx: &RenderCx) -> String {
    let mut out = text.to_string();
    for b in buttons {
        let line = match b {
            Button::MiniApp { label, .. } if cx.mini_app_base_url.is_none() => {
                Some(mini_app_unavailable(cx, label))
            }
            Button::Url { label, .. } if b.safe_url().is_none() => {
                Some(link_unavailable(cx, label))
            }
            _ => None,
        };
        if let Some(line) = line {
            out.push_str(&format!("\n\n{line}"));
        }
    }
    out
}

fn button(b: &Button, cx: &RenderCx) -> Option<Value> {
    match b {
        Button::Action { label, action } => {
            Some(json!({ "text": label, "callback_data": action.as_str() }))
        }
        // web_app works in private chats only, which is where Students are.
        Button::MiniApp { label, path } => cx
            .mini_app_url(path)
            .map(|url| json!({ "text": label, "web_app": { "url": url } })),
        // An inline `url` button; a URL that is not https is refused (the
        // notice line above stands in for it).
        Button::Url { label, .. } => b
            .safe_url()
            .map(|url| json!({ "text": truncate(&strip_format_chars(label), URL_LABEL_MAX), "url": url })),
        // #[non_exhaustive]: a kind this adapter does not know yet.
        _ => None,
    }
}

/// One button per row. The first ≤ 100 go on the last text message; more
/// go on follow-up messages "More options (2/3)".
fn attach_keyboards(out: &mut Vec<Value>, buttons: &[Button], cx: &RenderCx) {
    let rows: Vec<Value> = buttons
        .iter()
        .filter_map(|b| button(b, cx))
        .map(|b| json!([b]))
        .collect();
    if rows.is_empty() {
        return;
    }
    let chunks: Vec<&[Value]> = rows.chunks(KEYBOARD_BUTTONS_MAX).collect();
    let n = chunks.len();
    for (i, chunk) in chunks.into_iter().enumerate() {
        let markup = json!({ "inline_keyboard": chunk });
        if i == 0 {
            out.last_mut().expect("at least one text message")["reply_markup"] = markup;
        } else {
            let mut v = send_message(json!(more_label(cx, i + 1, n)), false);
            v["reply_markup"] = markup;
            out.push(v);
        }
    }
}

fn attachment(a: &Attachment, cx: &RenderCx) -> Value {
    let caption = truncate(&a.file_name, CAPTION_MAX);
    let is_photo = matches!(
        a.content_type.as_str(),
        "image/jpeg" | "image/png" | "image/webp"
    );
    let (method, field) = if is_photo && a.size_bytes <= PHOTO_UPLOAD_MAX {
        ("sendPhoto", "photo")
    } else {
        ("sendDocument", "document")
    };
    let by_url = if is_photo && method == "sendPhoto" {
        PHOTO_BY_URL_MAX
    } else {
        FILE_BY_URL_MAX
    };
    if a.size_bytes <= by_url {
        return json!({ "method": method, field: a.url, "caption": caption });
    }
    if a.size_bytes <= FILE_UPLOAD_MAX {
        // Too big for Telegram to fetch by URL: `send` uploads the bytes as
        // multipart part "file" (read from the FileStore by `file_key`).
        return json!({ "method": method, field: "attach://file", "caption": caption, "x_upload_file_key": a.file_key });
    }
    send_message(
        json!(file_link(cx, &a.file_name, a.size_bytes, &a.url)),
        false,
    )
}

/// A command name from an Action id: lowercase `[a-z0-9_]`, ≤ 32 chars
/// (<https://core.telegram.org/bots/api#botcommand>). The inbound "/name" is
/// mapped back through the same Menu.
pub fn command_name(action: &str) -> String {
    let s: String = action
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    s.chars().take(COMMAND_MAX).collect()
}

/// One Bot API command built from a Menu entry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Command {
    /// The command without `/`, e.g. `news_latest`.
    pub command: String,
    /// What the Student sees in the command list.
    pub description: String,
    /// The Action the command triggers.
    pub action: ActionId,
}

/// The commands for `m`, in order: names derived from the Action ids
/// ([`command_name`]), duplicates suffixed `_2`, `_3`; over
/// [`COMMANDS_MAX`] entries, the first 99 plus `/kernel_menu_more`. Both
/// [`menu`] and the inbound command map use this, so they always agree.
pub fn commands(m: &Menu) -> Vec<Command> {
    let mut entries: Vec<(String, ActionId)> = m
        .entries
        .iter()
        .map(|e| (e.label.clone(), e.action.clone()))
        .collect();
    if entries.len() > COMMANDS_MAX {
        entries.truncate(COMMANDS_MAX - 1);
        let more = ActionId::new(MENU_MORE_ACTION).expect("the Kernel's id is short");
        entries.push(("เพิ่มเติม".into(), more));
    }
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .map(|(label, action)| {
            let base = command_name(action.as_str());
            let mut name = base.clone();
            let mut k = 2;
            while !seen.insert(name.clone()) {
                let suffix = format!("_{k}");
                name = format!(
                    "{}{suffix}",
                    &base[..base.len().min(COMMAND_MAX - suffix.len())]
                );
                k += 1;
            }
            Command {
                command: name,
                description: truncate(&label, COMMAND_DESCRIPTION_MAX),
                action,
            }
        })
        .collect()
}

/// `setMyCommands` (scope = this chat) + `setChatMenuButton` (opens the
/// command list). `set_menu` fills both `chat_id`s and switches the button
/// to the Web App when one is configured.
pub fn menu(m: &Menu) -> Value {
    let commands: Vec<Value> = commands(m)
        .into_iter()
        .map(|c| json!({ "command": c.command, "description": c.description }))
        .collect();
    json!({
        "setMyCommands": { "method": "setMyCommands", "commands": commands, "scope": { "type": "chat" } },
        "setChatMenuButton": { "method": "setChatMenuButton", "menu_button": { "type": "commands" } },
    })
}
