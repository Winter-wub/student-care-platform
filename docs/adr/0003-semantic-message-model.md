---
status: accepted
date: 2026-11-04
---

# Semantic message model across channels

The Core must reach students on LINE and Telegram (and later channels) without being written against either API. Modules build outgoing messages from channel-neutral concepts (Card, Card List, Choices, Menu, Text, Attachment); each Channel adapter renders them in its own format and degrades what it cannot show by rules the adapter owns. We rejected the lowest-common-denominator approach (text plus buttons only, wastes LINE Flex and Telegram inline keyboards) and per-channel hand-written templates (every new message means work in every channel).

## Consequences

- A Module never sees channel-specific payloads; if a Module needs something the model lacks, the model grows.
- Adapter rendering rules (list caps, button limits, edit versus re-send) are decided per channel and documented with the adapter.

## Amendment (#108, 2026-11-08): URL buttons

The first slice left out URL buttons; a Deployment now needs links from an Announcement to its own web pages. `Button` gains `Url { label, url }`, built through `Button::url`, which refuses any URL but one LINE and Telegram both accept (`render::link_url`): `https:`, at most 1,000 bytes (LINE's URI action limit), printable ASCII without quotes, angle brackets, square brackets or backslashes, valid `%XX` escapes, at most one `#`, no user info, and a host of at most 253 bytes and two or more labels, none empty, over 63 bytes, or starting or ending with `-`, the last not all digits. The label loses invisible characters (Unicode format characters such as bidi controls and zero-width, and blank fillers such as U+3164 and U+2800), U+2028 and U+2029 become spaces, and a label left blank becomes the URL's host (`render::label_or_host`): LINE and Telegram refuse a whole message for an empty button label. `Button` was already `#[non_exhaustive]`, so adding the variant breaks no Module, and the variant is `#[non_exhaustive]` too. The constructor is not the guard, though: the `fixtures` feature builds a raw one for tests, and a `&mut Button` lets any code rewrite `url` after construction. The real guard is each adapter's re-check (`Button::safe_url`, the same `link_url`) right before rendering: a refused URL never reaches a payload.

Degrade rules:

| | LINE | Telegram | Fake |
|---|---|---|---|
| Card / Card List button | Flex button with a `uri` action, label cut to 40 | inline keyboard `url` button, label cut to 40 (ours; the Bot API states no limit) | recorded as sent |
| Choices option | quick-reply `uri` action, label 20; options that do not fit become a bubble as before | inline keyboard `url` button | recorded as sent |
| URL refused (fails `link_url`; never from `Button::url`) | no button; a grey line "(ลิงก์ “label” เปิดไม่ได้)" in its place (`render::link_unavailable`) | no button; the same line appended to the text | `send` fails with `Error::Invalid`, nothing recorded |

A URL button carries no Action, so the rule that no Action is dropped is unchanged; Action buttons beside a refused URL stay.

The adapters clean labels the same way (blank becomes the host) and cut them to the limits above.

An Announcement's body is a small Markdown subset (`harpseal::rich_text`); a Card body stays plain text. A link that fails `link_url` is text, so it never becomes a button, and a Push is never refused because of a URL. The pushed Card shows the body with the markup removed and adds a URL button for each of the body's first two distinct links after the Mini App button. The Mini App shows the format.

## Amendment (#109, 2026-11-08): a Card image

Announcements carry images, and the first belongs at the top of the pushed Card. `Card` gains `image: Option<CardImage>`: an `https:` URL the platform fetches itself, without a session, and an alt text. `CardImage::new` refuses a URL that fails `render::link_url`, as `Button::url` does; every adapter checks again (`CardImage::safe_url`) and leaves a refused image out. `Card` is a plain struct, so adding the field breaks every `Card { .. }` literal outside the Core; a Module adds `image: None`. The Card's text never depends on the image: it reads the same with the image left out.

The Core serves the image itself on an unguessable public path (`/announcements/files/<token>`, 128 random bits), only while the Announcement is sent and live, and builds the absolute URL from the Deployment's `public_url`. Without `public_url` a Card has no image.

Degrade rules:

| | LINE | Telegram | Fake |
|---|---|---|---|
| Card / Card List with an image | the Flex bubble's `hero`: `size: full`, `aspectRatio: 20:13`, `aspectMode: cover`; one message, no extra quota | the first text message's `link_preview_options` (`url`, `prefer_large_media`, `show_above_text`): the image as a large preview above the text, still one message, so `editMessageText` replaces it and `deleteMessage` removes it like any Card; Telegram builds the preview itself and may show none, and the text stands alone then | recorded as sent |
| A long body sent as Text first (LINE, over 2,000 characters) | the hero goes on the bubble that keeps the title and buttons | (no such split) | — |
| Image URL refused (fails `link_url`; never from `CardImage::new`) | no hero | no preview options | `send` fails with `Error::Invalid`, nothing recorded |

Telegram was given a link preview rather than `sendPhoto` with the text as caption (captions stop at 1,024 characters, and a photo message cannot be edited into a text one) or a photo before the Card (two messages to edit and delete, and the image would stay behind after an edit that removes it).

## Amendment (#113, 2026-11-09): headings, colours and PDFs

The body format gains `## ` and `### ` headings and a fixed palette of text colours (`{red}…{/red}`; `red`, `orange`, `green`, `blue`, `grey`). Neither reaches chat: `Doc::plain` turns a heading into a plain line and drops colours, so a Card body stays plain text on every Channel and no adapter changes. An Announcement may also carry up to 3 PDFs. The Card gives the first one a `Button::url` labelled with the file name, pointing at its public path under `public_url`, inside the same budget of two URL buttons (the body's links first, then the PDF; with a PDF, one link). Without `public_url` no PDF button is sent and the links keep both places. The adapters' URL re-check and label rules apply to it unchanged. The PDF button is `Button::external`, a new field of the `#[non_exhaustive]` `Url` variant (default `false`): it asks for the phone's own browser. LINE renders it by appending `openExternalBrowser=1` to the URI (a sandboxed PDF shows blank in WebKit before Safari 18.4); Telegram and the Fake Channel ignore it, as Telegram offers no per-link choice. A PDF URL that `Button::url` refuses leaves the button out, never the Card.
