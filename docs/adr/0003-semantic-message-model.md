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

The first slice left out URL buttons; a Deployment now needs links from an Announcement to its own web pages. `Button` gains `Url { label, url }`, built through `Button::url`, which refuses any URL but one LINE and Telegram both accept (`render::link_url`): `https:`, at most 1,000 bytes (LINE's URI action limit), printable ASCII without quotes, angle brackets, square brackets or backslashes, valid `%XX` escapes, at most one `#`, no user info, and a host of two or more labels, none empty or starting or ending with `-`. It also strips Unicode format characters (bidi controls, zero-width) from the label. `Button` was already `#[non_exhaustive]`, so adding the variant breaks no Module, and the variant is `#[non_exhaustive]` too. The constructor is not the guard, though: the `fixtures` feature builds a raw one for tests, and a `&mut Button` lets any code rewrite `url` after construction. The real guard is each adapter's re-check (`Button::safe_url`, the same `link_url`) right before rendering: a refused URL never reaches a payload.

Degrade rules:

| | LINE | Telegram | Fake |
|---|---|---|---|
| Card / Card List button | Flex button with a `uri` action, label cut to 40 | inline keyboard `url` button, label cut to 40 (ours; the Bot API states no limit) | recorded as sent |
| Choices option | quick-reply `uri` action, label 20; options that do not fit become a bubble as before | inline keyboard `url` button | recorded as sent |
| URL refused (fails `link_url`; never from `Button::url`) | no button; a grey line "(ลิงก์ “label” เปิดไม่ได้)" in its place (`render::link_unavailable`) | no button; the same line appended to the text | `send` fails with `Error::Invalid`, nothing recorded |

A URL button carries no Action, so the rule that no Action is dropped is unchanged; Action buttons beside a refused URL stay.

Labels are cut to the limits above and lose their format characters in the adapters as well.

An Announcement's body is a small Markdown subset (`harpseal::rich_text`); a Card body stays plain text. A link that fails `link_url` is text, so it never becomes a button, and a Push is never refused because of a URL. The pushed Card shows the body with the markup removed and adds a URL button for each of the body's first two distinct links after the Mini App button. The Mini App shows the format.
