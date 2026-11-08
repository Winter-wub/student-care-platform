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

The first slice left out URL buttons; a Deployment now needs links from an Announcement to its own web pages. `Button` gains `Url { label, url }`, built only through `Button::url`, which refuses anything but an `https:` URL with a host and no user info, made of printable ASCII without quotes, angle brackets or backslashes, at most 1,000 bytes (`render::link_url`; LINE's URI action limit). The variant is `#[non_exhaustive]`, so no code outside the Core can build one past that check; `Button` itself was already `#[non_exhaustive]`, so adding the variant breaks no Module. Adapters check the URL again (`Button::safe_url`) and never put a refused one into a payload.

Degrade rules:

| | LINE | Telegram | Fake |
|---|---|---|---|
| Card / Card List button | Flex button with a `uri` action, label cut to 40 | inline keyboard `url` button, label cut to 40 (ours; the Bot API states no limit) | recorded as sent |
| Choices option | quick-reply `uri` action, label 20; options that do not fit become a bubble as before | inline keyboard `url` button | recorded as sent |
| URL refused (not https, only buildable inside the Core) | no button; the grey "cannot be opened" line in its place | no button; the same line appended to the text | `send` fails with `Error::Invalid`, nothing recorded |

A URL button carries no Action, so the rule that no Action is dropped is unchanged; Action buttons beside a refused URL stay.

An Announcement's body is a small Markdown subset (`harpseal::rich_text`); a Card body stays plain text. The pushed Card shows the body with the markup removed and adds a URL button for each of the body's first two distinct links after "เปิดดูรายละเอียด". The Mini App shows the format.
