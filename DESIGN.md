---
name: harpseal
description: Back office and Mini App of a student care Core, branded per institution.
colors:
  harbour-teal: "#0f5c7a"
  on-brand: "#ffffff"
  counter-mist: "#f4f6f8"
  paper: "#ffffff"
  ledger-ink: "#17212b"
  slate-note: "#5b6b7a"
  rule-line: "#dde3e9"
  field-edge: "#b8c2cc"
  chip-fog: "#e8eef3"
  chip-ink: "#2c4456"
  approved-wash: "#e5f5ec"
  approved-ink: "#145c36"
  refused-wash: "#fdecec"
  refused-ink: "#8a1c1c"
  caution-wash: "#fff3d6"
  caution-ink: "#6b4a00"
  caution-meter: "#c98a00"
  over-meter: "#c62828"
  text-red: "#b42318"
  text-orange: "#b54708"
  text-green: "#067647"
  text-blue: "#175cd3"
  text-grey: "#475467"
typography:
  headline:
    fontFamily: "system-ui, -apple-system, Segoe UI, Noto Sans Thai, Sukhumvit Set, sans-serif"
    fontSize: "1.375rem"
    fontWeight: 700
    lineHeight: 1.3
  title:
    fontFamily: "system-ui, -apple-system, Segoe UI, Noto Sans Thai, Sukhumvit Set, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 700
  body:
    fontFamily: "system-ui, -apple-system, Segoe UI, Noto Sans Thai, Sukhumvit Set, sans-serif"
    fontSize: "16px"
    fontWeight: 400
    lineHeight: 1.6
  label:
    fontFamily: "system-ui, -apple-system, Segoe UI, Noto Sans Thai, Sukhumvit Set, sans-serif"
    fontSize: "1rem"
    fontWeight: 600
  caption:
    fontFamily: "system-ui, -apple-system, Segoe UI, Noto Sans Thai, Sukhumvit Set, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
rounded:
  chip: "999px"
  meter: "6px"
  control: "10px"
  card: "12px"
  bubble: "14px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  gutter: "16px"
  tap: "48px"
components:
  button-primary:
    backgroundColor: "{colors.harbour-teal}"
    textColor: "{colors.on-brand}"
    rounded: "{rounded.control}"
    padding: "0 20px"
    height: "48px"
  button-secondary:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.harbour-teal}"
    rounded: "{rounded.control}"
    padding: "0 20px"
    height: "48px"
  button-done:
    backgroundColor: "{colors.approved-wash}"
    textColor: "{colors.approved-ink}"
    rounded: "{rounded.control}"
    height: "48px"
  input:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.ledger-ink}"
    rounded: "{rounded.control}"
    padding: "10px 12px"
    height: "48px"
  card:
    backgroundColor: "{colors.paper}"
    rounded: "{rounded.card}"
    padding: "{spacing.gutter}"
  chip:
    backgroundColor: "{colors.chip-fog}"
    textColor: "{colors.chip-ink}"
    rounded: "{rounded.chip}"
    padding: "2px 10px"
  flash-ok:
    backgroundColor: "{colors.approved-wash}"
    textColor: "{colors.approved-ink}"
    rounded: "{rounded.control}"
    padding: "12px 14px"
  flash-err:
    backgroundColor: "{colors.refused-wash}"
    textColor: "{colors.refused-ink}"
    rounded: "{rounded.control}"
    padding: "12px 14px"
  top-bar:
    backgroundColor: "{colors.harbour-teal}"
    textColor: "{colors.on-brand}"
    padding: "8px 16px"
---

# Design System: harpseal

## Overview

**Creative North Star: "The Registrar's Counter"**

The back office is a well-run registrar's counter. Papers are stacked neatly, every form is checked before it is stamped, and the institution's colour appears only on the letterhead. Everything else stays out of the way. The system is flat, quiet and orderly. Paper-white cards sit on a cool mist ground, separated by thin rules, never by shadows. One brand colour does the speaking: it fills the top bar and the primary buttons, then steps back so content and status carry the page.

It is built first for non-technical Staff, and it works on a phone. Tap targets are 48 px everywhere. Tables become stacked cards below 900 px. Nothing scrolls sideways at 360 px. The same stylesheet serves the Mini App, narrowed to one 560 px column, so a Student's page and a Staff page feel like the same institution. The brand colour is a per-Deployment variable (`--brand`), so the system must look right in any institution's colour, not only in Harbour Teal.

This document records the incumbent system as of the start of the back-office revamp (#116). It is the reference to improve on, not a ceiling.

**Key Characteristics:**
- Calm and trustworthy: flat surfaces, thin rules, a single accent.
- Easy on a phone: 48 px targets, stacked tables, one-column Mini App.
- Institution-led colour: the brand is a variable; status colours are fixed.
- System fonts with Thai fallbacks; no web fonts.

## Colors

A cool, low-contrast neutral field with one institutional accent, plus three fixed status pairs (approved, refused, caution). Each pair is a pale wash with a deep ink.

### Primary
- **Harbour Teal** (#0f5c7a): the default institution colour, replaced per Deployment through `--brand`. It fills the top bar, primary buttons, links, progress bars and the Mini App avatar. Its 35%-alpha form draws the focus ring.

### Neutral
- **Counter Mist** (#f4f6f8): the page ground behind every card.
- **Paper** (#ffffff): cards, inputs, secondary buttons, preview bubbles.
- **Ledger Ink** (#17212b): body text and headings.
- **Slate Note** (#5b6b7a): secondary text, table captions, definition terms, meta lines.
- **Rule Line** (#dde3e9): card borders, list dividers, meter tracks.
- **Field Edge** (#b8c2cc): the resting border of inputs, selects and textareas.
- **Chip Fog / Chip Ink** (#e8eef3 / #2c4456): neutral chips and tags.

### Status
- **Approved Wash / Approved Ink** (#e5f5ec / #145c36): success flashes, "done" buttons, active Links, the meter under its limit.
- **Refused Wash / Refused Ink** (#fdecec / #8a1c1c): error flashes, blocking warnings, invalid fields, error chips.
- **Caution Wash / Caution Ink** (#fff3d6 / #6b4a00): quota warnings, override chips. The caution meter fill is #c98a00, and the over-limit fill is #c62828.

### Text palette (#113)
The only colours Staff can give text in an Announcement body (`{red}…{/red}`), as CSS classes `tc-red` … `tc-grey`. Fixed, never per Deployment, never a free hex. Each passes 4.5:1 on Paper and on Counter Mist; there is no dark theme, so a dark surface would need its own set.

| Token | Hex | On Paper #ffffff | On Counter Mist #f4f6f8 |
|---|---|---|---|
| text-red | #b42318 | 6.57:1 | 6.07:1 |
| text-orange | #b54708 | 5.43:1 | 5.01:1 |
| text-green | #067647 | 5.69:1 | 5.25:1 |
| text-blue | #175cd3 | 5.99:1 | 5.53:1 |
| text-grey | #475467 | 7.69:1 | 7.10:1 |

### Named Rules
**The Letterhead Rule.** The institution colour lives on the letterhead (top bar) and on the one action that moves the work forward. It never fills large areas or decorates.

**The Fixed Status Rule.** Approved, refused and caution colours never follow the brand. A red institution must still read "refused" in Refused Ink, not in its own red.

## Typography

**Body Font:** the platform system UI font, falling back to Noto Sans Thai and Sukhumvit Set for Thai.

**Character:** one native family at a few sizes, so Thai and Latin render with the device's own hinting. Hierarchy comes from weight and size, not typeface contrast.

### Hierarchy
- **Headline** (700, 1.375rem, 1.3): page titles (`h1`).
- **Title** (700, 1.125rem): card and section titles (`h2`). Subsections (`h3`) use 1rem.
- **Body** (400, 16px, 1.6): everything else. The generous line height leaves room for Thai tone marks and upper vowels.
- **Label** (600, 1rem): form labels, list names, buttons, flashes.
- **Caption** (400, 0.875rem): table headers, meta lines, the "who" line, definition terms. Small print goes down to 0.8125rem for chips and footers.

### Named Rules
**The Tall Line Rule.** Body line height never drops below 1.6. Thai stacked marks need the room.

## Layout

- **Containers:** a single centred column. `main` is at most 960px wide with a 16 px gutter that respects safe-area insets. The Mini App narrows to 560px.
- **Two-column pages:** from 900 px, `.cols` splits 3:2. Form pairs (`.row2`) sit side by side from 640 px.
- **Spacing:** 4, 8, 12 and 16 px steps. 16 px is the gutter and the card padding.
- **Responsive tables:** `table.rows` are stacked cards with an inline caption per cell (`data-label`) below 900 px, and a real table above.
- **Dense lists:** use `.list`, divided by top rules, rather than tables.
- **Home tiles:** an auto-fill grid with a minimum width of 220 px.

**The No-Sideways Rule.** No page scrolls horizontally at 360 px. Lists and stacked tables exist so that it never has to.

## Elevation & Depth

Flat. There are no shadows anywhere. Depth comes from tone (Paper on Counter Mist) and from 1 px Rule Line borders. A state is shown with colour washes (flash, chip, row highlight #fff8e6), never with lift.

**The Flat Counter Rule.** Surfaces sit flat on the counter. If something needs attention, colour it; do not raise it.

## Shapes

Softly rounded, never pill-shaped except for chips.

| Element | Radius |
|---|---|
| Cards, fieldsets, tables (desktop), tiles | 12 px |
| Buttons, inputs, flashes | 10 px |
| Preview bubbles | 14 px |
| Meters | 6 px |
| Chips | fully round |

Borders are always 1 px.

## Components

Sturdy and plain: big, obvious, forgiving controls. The look of a well-designed official form.

### Buttons
- **Shape:** gently rounded (10px), at least 48 × 48 px, label 600 weight.
- **Primary:** Harbour Teal (the brand) fill with white text, 0 20px padding.
- **Secondary:** Paper fill with brand text and a brand border.
- **Done:** Approved Wash with Approved Ink, for completed states such as an acknowledgement.
- **Link button:** text only and underlined, for low-weight actions inside lists.
- **Focus:** a 3px ring in the brand colour at 35% alpha, 1px offset.

### Cards / Containers
- **Corner Style:** 12px. **Background:** Paper. **Border:** 1px Rule Line. **Padding:** 16px. **Shadow:** none.

### Inputs / Fields
- **Style:** Paper background, 1px Field Edge border, 10px radius, 48px minimum height, 10px 12px padding. Textareas are at least 120px tall.
- **Focus:** the brand focus ring, and the border turns brand.
- **Error:** the border turns Refused Ink (`aria-invalid`), with a 600-weight Refused Ink message below.

### Chips
- **Style:** fully round, 2px 10px padding, 0.8125rem. Neutral (Chip Fog), plus approved, refused and caution variants.
- **Clickable chips** grow to the 48px tap height.

### Navigation
- **Top bar:** a brand-coloured band holding the institution name (with an optional logo on a white tile), the nav links (12px 10px, 48px tall, white at 12% alpha on hover) and the signed-in "who" line.
- **Mobile:** the nav wraps beneath the name.

### Flash and banners
- **Flash:** a full-width wash block (approved or refused) in label weight.
- **Usage banner:** a strip under the bar in caution or refused colours.

### Responsive data table (signature)
The `table.rows` pattern carries Staff's daily lists (Students, imports, audit). Below 900 px each row becomes a bordered card and each cell shows its column name as a caption. Above 900 px it is a single bordered table with muted headers.

### Message preview bubble (signature)
The announcement preview draws a chat bubble: Paper, 14px radius, at most 22rem wide. Staff see roughly what Students will get before they send.

## Do's and Don'ts

### Do:
- **Do** use `--brand` for the top bar, the primary action, links and focus. Assume it can be any institution colour.
- **Do** keep every interactive element at least 48px tall and wide.
- **Do** separate surfaces with Rule Line borders and tone, not shadows.
- **Do** use the fixed status pairs (Approved, Refused, Caution) for every state message.
- **Do** keep body line height at 1.6 for Thai.
- **Do** use `table.rows` or `.list` for collections, so phones get stacked cards.

### Don't:
- **Don't** add box shadows or elevation.
- **Don't** colour status messages with the brand colour.
- **Don't** let any page scroll sideways at 360px.
- **Don't** fill large areas with the brand colour beyond the top bar.
- **Don't** show the harpseal brand on Student-facing surfaces (chat, Mini App).
