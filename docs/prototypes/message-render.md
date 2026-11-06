# Prototype: semantic message model on LINE and Telegram (issue #3)

Throwaway. Built on Design A (issue #4), Design B removed. Renderers are pure functions returning
`serde_json::Value`; no platform is called. `Channel::send` renders, then stops at `todo!()` before HTTP.

- LINE: `harpseal_channel_line::render::{message, menu}`, plus `requests` (packs ≤ 5 objects per call).
- Telegram: `harpseal_channel_telegram::render::{message, menu}`, plus `calls` (adds `chat_id`).
- Shared: `harpseal::render::{RenderCx, split_text, truncate, action_ids}`. `RenderCx { locale, mini_app_base_url }`.
- Fixtures: `crates/harpseal/src/fixtures.rs` (feature `fixtures`, dev-only). Payloads:
  [`message-render/line/`](message-render/line/) and [`message-render/telegram/`](message-render/telegram/),
  rewritten by `cargo test --workspace` (print them with `-- --nocapture`). Files `x_*` show degrade rules.
- Tests check every limit below on every fixture, and that each Action id appears exactly once in the output
  (no Action is ever dropped).

Model changes made to get here: `Attachment` gained `size_bytes` and `url` (a signed HTTPS URL the Kernel mints
from the FileStore), and `Channel::send` / `EditMessage::edit` gained `locale: &Locale` (degrade texts are in the
Student's language; `send` had no way to know it).

## Mapping

| Model | LINE | Telegram |
|---|---|---|
| `Text` | `text` message | `sendMessage` (plain, no `parse_mode`) |
| `Card` | Flex `bubble`: bold title + wrapped body, buttons in `footer` (first `primary`) | `sendMessage`, `parse_mode: HTML`, `<b>title</b>\nbody` (escaped `& < >`), `InlineKeyboardMarkup` one button per row |
| `CardList` | Flex `carousel` of bubbles | one `sendMessage` per Card, in order (no carousel) |
| `Choices` | `text` + `quickReply` items | `sendMessage` + inline keyboard |
| `Attachment` | `image` if JPEG/PNG ≤ 1 MB, else `text` with name, size, link | `sendPhoto` / `sendDocument` by URL, by upload (`attach://file`), or `text` with link |
| `Menu` | rich menu object, postback `areas` on a 2500 px grid | `setMyCommands` (scope `chat`) + `setChatMenuButton` (`commands`) |
| `Button::Action` | `postback`, `data` = ActionId, `displayText` = full label | `callback_data` = ActionId |
| `Button::MiniApp` | `uri` = LIFF URL + path | `web_app: { url }` = Web App URL + path |
| `MenuEntry` | area `postback` | `BotCommand` named from the ActionId (`news.latest` → `/news_latest`) |

Core work the renderers expose: LINE rich menus need an image (JPEG/PNG, 2500 × 843 or 1686, ≤ 1 MB) uploaded to
`POST api-data.line.me/v2/bot/richmenu/{id}/content` before linking
([ref](https://developers.line.biz/en/reference/messaging-api/#upload-rich-menu-image)); the model has no image, so
someone must supply or draw one per Menu. Telegram commands come back as plain text `/news_latest`; the adapter must map
them to `Inbound::Action` through the Student's current Menu.

## LINE degrade rules

Limits from the [Messaging API reference](https://developers.line.biz/en/reference/messaging-api/). Lengths are counted in
characters (`char`s); LINE does not say how it counts Thai, so treat that as unconfirmed.

| Case | Limit (cited) | Default (what the Student sees) | Alternative |
|---|---|---|---|
| Card List > 12 | 12 bubbles, 50 KB per carousel ([#f-carousel](https://developers.line.biz/en/reference/messaging-api/#f-carousel)) | Several carousels of ≤ 12, split greedily by count and JSON size; altText "12 รายการ (1/2): …" | 11 Cards + a "ดูทั้งหมด" Card opening a Mini App list |
| > 5 message objects in one send | 5 per request ([#send-reply-message](https://developers.line.biz/en/reference/messaging-api/#send-reply-message)) | First 5 go in the Reply, the rest as Push. That part is **metered** | Refuse > 5 at the Core |
| Choices > 13 or a label > 20 | 13 items ([#items-object](https://developers.line.biz/en/reference/messaging-api/#items-object)), label 20 ([#action-object-label-spec](https://developers.line.biz/en/reference/messaging-api/#action-object-label-spec)) | The whole Choices becomes one bubble with every option as a button (label 40) | First 12 quick replies + "more" Action |
| Flex button label > 40 | 40 ([#action-object-label-spec](https://developers.line.biz/en/reference/messaging-api/#action-object-label-spec)) | Cut to 39 + "…"; `displayText` (≤ 300) echoes the full label after the tap | Reject at build time |
| Text > 5000 | 5000 ([#text-message](https://developers.line.biz/en/reference/messaging-api/#text-message)) | Split into several texts at a newline/space; quick reply stays on the last | — |
| Card body long | bubble 30 KB ([#bubble](https://developers.line.biz/en/reference/messaging-api/#bubble)); no per-text cap | Body > 2000 chars (our cap): full text first as Text, then a bubble with title + buttons. In a carousel the body is cut to 2000 + "…" | Always cut |
| altText > 1500 | 1500 ([#flex-message](https://developers.line.biz/en/reference/messaging-api/#flex-message)) | Cut with "…" | — |
| postback `data` > 300 | 300 ([#postback-action](https://developers.line.biz/en/reference/messaging-api/#postback-action)) | Cannot happen: `ActionId` is ≤ 64 bytes (compile-time assert) | — |
| Attachment | image 10 MB, preview 1 MB ([#image-message](https://developers.line.biz/en/reference/messaging-api/#image-message)); no file message type for bots | JPEG/PNG ≤ 1 MB → `image` (same URL as preview). Anything else → text "ไฟล์แนบ: name (x MB) / ดาวน์โหลด: url" | Core makes a preview to send images up to 10 MB |
| Menu > 20 entries | 20 areas, chatBarText 14 ([#rich-menu-object](https://developers.line.biz/en/reference/messaging-api/#rich-menu-object)) | 19 entries + "เพิ่มเติม" → Kernel Action `kernel.menu.more` answers with the rest as Choices | Rich menu tabs (switch action) |
| `Button::MiniApp`, no LIFF id | — | No button; a grey line "(“label” ยังเปิดในช่องทางนี้ไม่ได้ ติดต่อเจ้าหน้าที่…)" in its place. Action buttons stay | `BuildError` when a Module uses Mini App buttons |

## Telegram degrade rules

Limits from the [Bot API](https://core.telegram.org/bots/api) (Bot API 10.3 at time of writing).

| Case | Limit (cited) | Default (what the Student sees) | Alternative |
|---|---|---|---|
| Card List (any size) | no carousel | One message per Card. 15 Cards = 15 messages, ≈ 15 s at the 1 msg/s per chat guidance ([FAQ](https://core.telegram.org/bots/faq#my-bot-is-hitting-limits-how-do-i-avoid-this)) | One message listing titles, buttons "1".."15" |
| Text > 4096 | 1-4096 after entity parsing ([#sendmessage](https://core.telegram.org/bots/api#sendmessage)) | Split; Card title stays bold in the first part; keyboard on the last | — |
| Inline keyboard > 100 buttons | **unconfirmed**: no count in the docs ([#inlinekeyboardmarkup](https://core.telegram.org/bots/api#inlinekeyboardmarkup)); 100 is our ceiling | First 100 on the message, then follow-ups "ตัวเลือกเพิ่มเติม (2/2)" with ≤ 100 each | Pages with "next" Action |
| `callback_data` > 64 bytes | 1-64 bytes ([#inlinekeyboardbutton](https://core.telegram.org/bots/api#inlinekeyboardbutton)) | Cannot happen: `ActionId::new` rejects > 64 bytes | — |
| HTML injection in Card text | `& < >` must be escaped ([#html-style](https://core.telegram.org/bots/api#html-style)) | Escaped by hand (`escape_html`); no new dependency | — |
| Attachment | by URL: photo 5 MB, other 20 MB ([#sending-files](https://core.telegram.org/bots/api#sending-files)); upload: photo 10 MB, other 50 MB ([#senddocument](https://core.telegram.org/bots/api#senddocument)) | ≤ URL limit → by URL; ≤ upload limit → `attach://file` and `send` uploads the FileStore bytes; > 50 MB → text with link | Always link |
| Menu > 100 entries | 100 commands, name 1-32 `[a-z0-9_]`, description 1-256 ([#setmycommands](https://core.telegram.org/bots/api#setmycommands), [#botcommand](https://core.telegram.org/bots/api#botcommand)) | 99 + `/kernel_menu_more`. Names derived from the ActionId, duplicates get `_2`, `_3` | — |
| `Button::MiniApp`, no Web App URL | `web_app` only in private chats ([#inlinekeyboardbutton](https://core.telegram.org/bots/api#inlinekeyboardbutton)) | No button; the same notice line is appended to the text. Action buttons stay | `BuildError` |

## Verdict

The six variants are enough for news + "รับทราบ" + email OTP + unlink. Every first-slice fixture renders on both
channels with no degrade. Candidates, judged against the first slice only:

| Candidate | Needed in slice 1? | Why |
|---|---|---|
| Image/thumbnail on Card | No | News reads fine as text; adds LINE `hero` and a different Telegram method (`sendPhoto` + caption ≤ 1024). Slice 2. |
| Per-Card footer/date | No | The date fits in the body. |
| `Message::Markdown`-like rich text | No | LINE text has no markup; bold title covers the need. Keep a plain model. |
| `Button::Url` | No, but soon | "เปิดดูรายละเอียด" is a Mini App. A link to the faculty website will be wanted next; trivial on both (`uri` / `url`). |
| "silent" flag | No | OTP and acknowledge are Replies; maps to `disable_notification` / `notificationDisabled` later. |
| `Menu` icons | No (model) / **Yes (LINE image)** | LINE needs one rich menu image per Menu; that belongs to the Deployment's LINE config, not the model. |
| Card with no buttons | Already works | `buttons` may be empty (tested). |
| Choices > 13 | No | The slice uses 2 options; the degrade handles more. |

Missing, found by building it:
1. **Locale on `send`**: added. Degrade texts have to follow the Student's Locale.
2. **One Reply per handler**: `Deployment::send` sends each Message separately with the same reply token, but a LINE
   token works once. The Kernel must send a handler's whole `Vec<Message>` as one batch (≤ 5 objects free, the rest Push).
3. **Telegram command → Action**: commands arrive as text; `parse_webhook` needs the Student's Menu to map them.
4. **Attachment URL and size**: added. Without them no adapter can choose a message type.
5. **Menu chat bar text**: `chatBarText` is hard-coded "เมนู"; a Menu title from the catalog would fix it.

Drop for now: outbound `Message::Attachment` is not used by the slice, and on LINE it is a link for anything but a
small image. Keep the variant, but take it out of the required Channel minimum until a Module needs it.

### Questions for the owner

1. Who makes the LINE rich menu images (locked and unlocked): a designer, once per Deployment, or should the Core draw
   labels on a plain image (needs an image library compiled to wasm)?
2. Card List > 12 on LINE: keep several carousels (default), or 11 Cards + "ดูทั้งหมด" in the Mini App? On Telegram, are
   15 separate messages acceptable, or should a long list be one message with numbered buttons?
3. Is a Mini App required in every Deployment? If yes, a Mini App button without a LIFF id / Web App URL should be a
   `BuildError`; if no, keep the visible notice (default) and add `Button::Url` as the fallback for "เปิดดูรายละเอียด".
