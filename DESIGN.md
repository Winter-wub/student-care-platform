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
  field-edge: "#7d8a97"
  chip-fog: "#e8eef3"
  chip-ink: "#2c4456"
  approved-wash: "#e5f5ec"
  approved-ink: "#145c36"
  refused-wash: "#fdecec"
  refused-ink: "#8a1c1c"
  refused-press: "#6f1515"
  caution-wash: "#fff3d6"
  caution-edge: "#a37200"
  attention-wash: "#fff8e6"
  caution-ink: "#6b4a00"
  caution-meter: "#9a6a00"
  approved-edge: "#3d8a5f"
  caution-text: "#8a5a00"
  refused-edge: "#c24141"
  over-meter: "#c62828"
  text-red: "#b42318"
  text-orange: "#b54708"
  text-green: "#067647"
  text-blue: "#175cd3"
  text-grey: "#475467"
typography:
  headline:
    fontFamily: "system-ui, -apple-system, Segoe UI, Noto Sans Thai, Sukhumvit Set, sans-serif"
    fontSize: "1.7578rem"
    fontWeight: 700
    lineHeight: 1.4
  section:
    fontFamily: "system-ui, -apple-system, Segoe UI, Noto Sans Thai, Sukhumvit Set, sans-serif"
    fontSize: "1.40625rem"
    fontWeight: 700
    lineHeight: 1.4
  title:
    fontFamily: "system-ui, -apple-system, Segoe UI, Noto Sans Thai, Sukhumvit Set, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 700
    lineHeight: 1.4
  body:
    fontFamily: "system-ui, -apple-system, Segoe UI, Noto Sans Thai, Sukhumvit Set, sans-serif"
    fontSize: "1rem"
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
  small:
    fontFamily: "system-ui, -apple-system, Segoe UI, Noto Sans Thai, Sukhumvit Set, sans-serif"
    fontSize: "0.8125rem"
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
  button-danger:
    backgroundColor: "{colors.refused-ink}"
    textColor: "{colors.on-brand}"
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
  nav-link:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.ledger-ink}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "48px"
  nav-link-current:
    backgroundColor: "{colors.harbour-teal}"
    textColor: "{colors.on-brand}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "48px"
  status-card-warn:
    backgroundColor: "{colors.attention-wash}"
    textColor: "{colors.ledger-ink}"
    padding: "{spacing.gutter}"
  tech-disclosure:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.slate-note}"
    rounded: "{rounded.card}"
    padding: "0 16px"
---

# Design System: harpseal

## Overview

**Creative North Star: "The Registrar's Counter"**

The back office is a well-run registrar's counter. Papers are stacked neatly, every form is checked before it is stamped, and the institution's colour appears only on the letterhead. Everything else stays out of the way. The system is flat, quiet and orderly. Paper-white cards sit on a cool mist ground, separated by thin rules, never by shadows. One brand colour does the speaking: it fills the top bar and the primary buttons, then steps back so content and status carry the page.

It is built first for non-technical Staff, and it works on a phone. Tap targets are 48 px everywhere. Tables become stacked cards when the content is narrower than 860 px. Nothing scrolls sideways at 360 px. The same stylesheet serves the Mini App, narrowed to one 560 px column, so a Student's page and a Staff page feel like the same institution. The brand colour is a per-Deployment variable (`--brand`), so the system must look right in any institution's colour, not only in Harbour Teal.

This document records the incumbent system as of the start of the back-office revamp (#116), updated as each step lands (#120 danger button, #123 navigation, status card and technical disclosure, #127 upload help, empty states, next step and first-run checklist). It is the reference to improve on, not a ceiling.

**Key Characteristics:**
- Calm and trustworthy: flat surfaces, thin rules, a single accent.
- Easy on a phone: 48 px targets, stacked tables, one-column Mini App.
- Institution-led colour: the brand is a variable; status colours are fixed.
- System fonts with Thai fallbacks; no web fonts.

## Colors

A cool, low-contrast neutral field with one institutional accent, plus three fixed status pairs (approved, refused, caution). Each pair is a pale wash with a deep ink.

### Primary
- **Harbour Teal** (#0f5c7a): the default institution colour, replaced per Deployment through `--brand`. It fills the top bar, primary buttons, progress bars, the current page and the Mini App avatar.
- **Derived brand tokens (#129, `ui::brand`):** computed per Deployment from the brand with WCAG relative luminance, never eyeballed. `--brand-ink` (text on the brand) is white, or Ledger Ink when white is under 4.5:1 (only a TOML colour can be: the branding page refuses one on save with the same function). `--brand-text` (links, secondary buttons) is the brand darkened in 5 % steps until it is at least 4.5:1 on every light surface (Paper, Counter Mist, Chip Fog and the washes). `--focus` (the focus ring, checkboxes, the chart) is the brand darkened until at least 3:1 on the same surfaces. Harbour Teal and the default #0b5cad need no darkening (6.2:1 and up); a saved colour at the 4.5:1 edge (#767676, 4.2:1 on Counter Mist) gets a darker `--brand-text`; a yellow TOML colour (#ffd400) gets Ledger Ink text on the bar and a dark-gold ring.

### Neutral
- **Counter Mist** (#f4f6f8): the page ground behind every card.
- **Paper** (#ffffff): cards, inputs, secondary buttons, preview bubbles.
- **Ledger Ink** (#17212b): body text and headings.
- **Slate Note** (#5b6b7a): secondary text, table captions, definition terms, meta lines.
- **Rule Line** (#dde3e9): card borders, list dividers, meter tracks.
- **Field Edge** (#7d8a97): the resting border of inputs, selects and textareas, the checklist ring and the empty preview outline: 3.5:1 on Paper, 3.3:1 on Counter Mist (was #b8c2cc, 1.81:1).
- **Caution Text** (#8a5a00): the unreachable Link state, field warnings and the Mini App's waiting line: caution as text on Paper (5.9:1) and Counter Mist (5.5:1).
- **Chip Fog / Chip Ink** (#e8eef3 / #2c4456): neutral chips and tags.

### Status
- **Approved Wash / Approved Ink** (#e5f5ec / #145c36): success flashes, "done" buttons, active Links, the meter under its limit.
- **Refused Wash / Refused Ink** (#fdecec / #8a1c1c): error flashes, blocking warnings, invalid fields, error chips, and the danger button's fill. **Refused Press** (#6f1515) is the danger button's hover.
- **Caution Wash / Caution Ink** (#fff3d6 / #6b4a00): quota warnings, override chips, the "ต้องดู" chip. The caution meter fill is #9a6a00 (3.7:1 on the Rule Line track; was #c98a00, 2.28:1), and the over-limit fill is #c62828 (4.4:1).
- **Status chip edges (#129):** Approved #3d8a5f (4.2:1 on Paper, 3.7:1 on its wash), Refused #c24141 (5.1:1 / 4.5:1), Caution #a37200 (4.2:1 / 3.8:1), 1px. Neutral chips are labels whose words carry them and stay borderless.
- **Attention Wash** (#fff8e6): the whole row that needs a look: a changed audit row, a Warn status card (#123). Ledger Ink on it is 15.4:1.

### Text palette (#113)
The only colours Staff can give text in an Announcement body (`{red}…{/red}`), as CSS classes `tc-red` … `tc-grey`. Fixed, never per Deployment, never a free hex. Each passes 4.5:1 on Paper and on Counter Mist; there is no dark theme, so a dark surface would need its own set. Colour adds emphasis, never meaning alone: coloured text is also semi-bold (600), and links keep their underline so Text Blue is never read as a link.

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
A 1.25 ratio between heading levels (#129; was 16/16/18/22 px, at most 1.22), built from the title step: `--step-title` 1.125rem, `--step-section` × 1.25, `--step-page` × 1.25.
- **Headline** (700, 1.7578rem ≈ 28 px, 1.4): page titles (`h1`), one per page.
- **Section** (700, 1.40625rem ≈ 22.5 px, 1.4): section and card titles (`h2`).
- **Title** (700, 1.125rem = 18 px, 1.4): subsections (`h3`), and the status card titles of the home ledger (an `h2` drawn at this step: a ledger row, not a section).
- **Body** (400, 1rem, 1.6): everything else, in the user's own font size (`100%`, not a fixed 16 px). The generous line height leaves room for Thai tone marks and upper vowels. Headings use 1.4: at 28 px, 1.3 left a wrapped Thai title's upper vowels nearly touching the line above. Headings wrap `balance`.
- **Figures:** tabular (`tabular-nums`) in tables, facts, counts, meters and status cards.
- **Label** (600, 1rem): form labels, list names, buttons, flashes.
- **Caption** (400, 0.875rem): table headers, meta lines, the "who" line, definition terms. Muted meta lines (`.muted`) use the caption size (was 0.9375rem, a step off the scale). Small print goes down to 0.8125rem for chips and footers.

### Named Rules
**The Tall Line Rule.** Body line height never drops below 1.6, headings never below 1.4. Thai stacked marks need the room.

**The font stays.** The system UI font with Noto Sans Thai and Sukhumvit Set fallbacks rendered every page without clipped marks at 360 px and on desktop (#129 audit), so no web font is loaded.

## Layout

- **Containers:** a single centred column. `main` is at most 960px wide with a 16 px gutter that respects safe-area insets. The Mini App narrows to 560px.
- **Content breakpoints are container queries on `main` (#123),** not the viewport, because the side navigation takes 15rem from 640 px. `main`'s content box is about the window minus 32 px without the navigation, minus 272 px with it.
- **Two-column pages:** from a 860 px content width, `.cols` splits 3:2. Form pairs (`.row2`) and `.facts` sit side by side from 600 px.
- **Spacing:** 4, 8, 12 and 16 px steps. 16 px is the gutter and the card padding.
- **Responsive tables:** `table.rows` are stacked cards with an inline caption per cell (`data-label`) below a 860 px content width (a 900 px window without the navigation, about 1130 px with it), and a real table above.
- **Dense lists:** use `.list`, divided by top rules, rather than tables.
- **Back-office shell:** from 640 px a 15rem navigation column and the content column (`main`, at most 960 px); below 640 px one column under the menu row.
- **Home:** the term status ledger (below), not a copy of the navigation.

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
- **Danger:** Refused Ink fill with white text (9.3:1), for an action in proportion to its reach: one that sends to every Student or makes many of them left (#120). The label names the reach ("ยืนยัน: 20 คนจะพ้นสภาพ", "ส่งถึงทุกคน 50 คน"). It never follows the brand, hovers darker (#6f1515) and has its own solid 3px Refused Ink focus ring at a 2px offset, so a red institution colour cannot hide it. Use `ui::danger_button`; a broad action above its threshold also asks Staff to type the reach, checked on the server.
- **Done:** Approved Wash with Approved Ink, for completed states such as an acknowledgement.
- **Link button:** text only and underlined, for low-weight actions inside lists.
- **Focus:** one solid 3px `--focus` ring at a 2px offset on every focusable element (links in text and tables included); on the bar and the current page it is `--brand-ink`. Was the brand at 35 % alpha (1.79:1).

### Cards / Containers
- **Corner Style:** 12px. **Background:** Paper. **Border:** 1px Rule Line. **Padding:** 16px. **Shadow:** none.

### Inputs / Fields
- **Style:** Paper background, 1px Field Edge border, 10px radius, 48px minimum height, 10px 12px padding. Textareas are at least 120px tall.
- **Focus:** the focus ring, and the border turns `--focus`.
- **Labels and errors (#129):** every control has a tied `label`, or a `legend` when it is one of a group (Audience attributes with listed values are a fieldset). A form with errors shows one Refused summary (`role="alert"`); each field's message has an id the control or fieldset names with `aria-describedby`, and is not a live region itself. A page announces one message per update: the flash, or its own message, not both.
- **Checkboxes and radios:** 20 px, `accent-color: var(--focus)`, inside a 48 px label.
- **Targets:** links in tables, lists, facts and the "back" line are at least 24 px tall; buttons and fields 48 px.
- **Error:** the border turns Refused Ink (`aria-invalid`), with a 600-weight Refused Ink message below.

### Chips
- **Style:** fully round, 2px 10px padding, 0.8125rem. Neutral (Chip Fog), plus approved, refused and caution variants.
- **Clickable chips** grow to the 48px tap height.

### Navigation (#123)
- **Skip link:** the first focusable element, "ข้ามไปยังเนื้อหา", off screen until focused, then a Paper tab with a solid brand ring over the bar. It jumps to `main#main`.
- **Top bar (the letterhead):** a brand-coloured band, one row, holding the institution name (with an optional logo on a white tile) and the signed-in "who" line with sign-out. Below 640 px the email hides so the bar stays one row (64 px).
- **Grouped navigation:** one `<details class="menu">` with a Home link, then two groups: **งานประจำ** (Students, the roster, announcements, every Module's daily pages) and **ผู้ดูแลระบบ** (Security Events, audit, Staff, Roles, texts, settings, usage, branding, backups). Group names are Caption-size Slate Note labels; a group the Staff member has no page in is not drawn. Links are Ledger Ink on Paper, 48 px tall, 10 px radius, Counter Mist on hover.
- **Current page:** `aria-current="page"` on the entry that owns the path (the longest parent, so `/admin/roster/123` marks the roster), drawn as the brand with On Brand text at 700: the letterhead's own pair, whose contrast the branding page checks on save (at least 4.5:1), so a light institution colour stays legible. Its focus ring is Ledger Ink at a 2px offset. No coloured side stripe.
- **Below 640 px:** the navigation folds behind a 48 px "เมนู" row on Paper (a drawn three-line icon, the word, then the current page in Slate Note). It opens without JS. Bar plus menu row is 113 px before the content on a 390 × 844 screen (was 392 px).
- **From 640 px:** a 15rem Paper side column with a Rule Line on its right, sticky to the top. Where the browser supports `::details-content` it is always open and the button is hidden; elsewhere the button stays and still works.
- **Brand:** the letterhead and the one current-page entry. The side column is Paper, never brand-filled (The Letterhead Rule).

### Flash and banners
- **Flash:** a full-width wash block (approved or refused) in label weight.
- **Usage banner:** a strip under the bar in caution or refused colours.

### Responsive data table (signature)
The `table.rows` pattern carries Staff's daily lists (Students, imports, audit). Below 900 px each row becomes a bordered card and each cell shows its column name as a caption. Above 900 px it is a single bordered table with muted headers.

### Status card (signature, #123)
The `/admin` home is "this term at a glance": one Paper card holding a ledger of status cards divided by Rule Lines, sorted by order. Each card is a **title with a state chip**, **one fact** (a short Thai sentence with the number that matters) and **one next action** (a secondary button). Kernel cards: the last roster import, announcements, the message quota, the last backup; Modules add their own (the example's fees card). Only cards the Staff member's Permissions allow are drawn.
- **Title:** from the card's catalog key, so a card that cannot be read is still named.
- **States:** Empty, a neutral chip "ยังไม่เริ่ม", whose fact says what to do first ("ยังไม่ได้นำเข้ารายชื่อ") and whose action starts it. Ok, an Approved chip "เรียบร้อย". Warn, a Caution chip "ต้องดู" with a 1px #a37200 border (4.0:1 on the wash) on the Attention Wash (#fff8e6). The state never uses the brand (The Fixed Status Rule); the wash is the only emphasis, never a shadow or a side stripe.
- **Layout:** stacked below a 720 px content width; from there three columns: title and chip (14rem), the fact, the action at the end.
- **Failure:** a card that cannot be read keeps its title, shows a Warn chip and "อ่านข้อมูลส่วนนี้ไม่ได้"; the rest renders.
- **Not a dashboard:** no big numbers, no sparklines, no tiles. One fact per card.

### Technical details disclosure (#123)
Administration pages keep raw details (cron, table names, IPs, request ids, Permission ids, config file names) behind a closed `details.tech` "ข้อมูลทางเทคนิค" at the end of the page: Paper, 1px Rule Line, 12px radius, a 48 px Slate Note summary, the body under a Rule Line. Cards inside it lose their own border so nothing nests.

### Upload help panel (#127)
Beside every file input that takes a spreadsheet: the roster's and each Module's (`ui::upload_help`). A Paper card in the `.cols` pair (form 3, help 2 from an 860 px content width; stacked below), titled "ไฟล์ที่ต้องเตรียม". It holds the file rules as a short bulleted list (CSV or XLSX, the size limit, CSV saved as UTF-8, how headers are matched), then the columns as a `.list`: the header in label weight, a chip ("ต้องมีค่า" neutral, "เว้นว่างได้" quiet: Paper with a Rule Line and Slate Note text), then what it holds and a fictional example on the line below. The template is a secondary button at the end. Never hidden behind a disclosure: the columns are the help.

### Empty state (#127)
A list with nothing yet is one Paper card: one sentence on what goes here (at most 60ch), then the one action that fills it as a primary button, drawn only when the Staff member holds its Permission. No illustration, no icon. A search or filter with no match is the same card with a sentence on what to try and a secondary "ล้างตัวกรอง" / "ล้างการค้นหา" link.

### Next step (#127)
After a confirmed import the result page ends with a "ขั้นต่อไป" card: one sentence on what the action does (and that nothing is sent before review), the primary "แจ้งนักศึกษา" for Staff who may send, and a secondary way back.

### First-run checklist (#127)
On the home, above the status ledger, while the Deployment is fresh (no Student yet): a Paper card "เริ่มต้นใช้งาน", a Slate Note line counting the steps left, and an ordered `.list` of steps. Each step is a drawn ring (2px Field Edge) and the step's name as a 48 px link, with a chip; a done step's ring fills Approved Ink with a white drawn check, its name turns Slate Note and its chip turns Approved. Not a progress bar, not a dismissable tour: it goes away by itself once the Deployment has Students.

### Message preview bubble (signature)
The announcement preview draws a chat bubble: Paper, 14px radius, at most 22rem wide. Staff see roughly what Students will get before they send. Before anything is typed, each preview holds one Slate Note line on what will appear there, in a 1px dashed Field Edge outline (#127), never an empty bubble.

## Do's and Don'ts

### Do:
- **Do** use `--brand` for the top bar and the primary action, `--brand-text` for links and `--focus` for the ring. Assume the brand can be any institution colour.
- **Do** compute a contrast before choosing a colour, and keep text at 4.5:1 and marks (borders, rings, meters) at 3:1.
- **Do** respect `prefers-reduced-motion`: the only motion, the HTMX spinner fade, is instant under it.
- **Do** keep every interactive element at least 48px tall and wide.
- **Do** separate surfaces with Rule Line borders and tone, not shadows.
- **Do** use the fixed status pairs (Approved, Refused, Caution) for every state message.
- **Do** keep body line height at 1.6 for Thai.
- **Do** use `table.rows` or `.list` for collections, so phones get stacked cards.
- **Do** give every home status card one state, one fact and one next action.
- **Do** put raw technical details behind the "ข้อมูลทางเทคนิค" disclosure.

### Don't:
- **Don't** add box shadows or elevation.
- **Don't** colour status messages with the brand colour.
- **Don't** let any page scroll sideways at 360px.
- **Don't** fill large areas with the brand colour beyond the top bar: the navigation column is Paper.
- **Don't** turn the home into tiles of the navigation or a wall of vanity numbers.
- **Don't** show the harpseal brand on Student-facing surfaces (chat, Mini App).
