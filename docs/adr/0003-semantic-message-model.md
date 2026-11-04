---
status: accepted
date: 2026-11-04
---

# Semantic message model across channels

The Core must reach students on LINE and Telegram (and later channels) without being written against either API. Modules build outgoing messages from channel-neutral concepts (Card, Card List, Choices, Menu, Text, Attachment); each Channel adapter renders them in its own format and degrades what it cannot show by rules the adapter owns. We rejected the lowest-common-denominator approach (text plus buttons only, wastes LINE Flex and Telegram inline keyboards) and per-channel hand-written templates (every new message means work in every channel).

## Consequences

- A Module never sees channel-specific payloads; if a Module needs something the model lacks, the model grows.
- Adapter rendering rules (list caps, button limits, edit versus re-send) are decided per channel and documented with the adapter.
