# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

**Primary: institution Staff in the back office.** Non-technical staff at a Thai university or college. There are three Roles:

- **Student Affairs Staff:** do most of the daily work. They import the roster and fee files, send announcements, follow up on fees, and manage grades and the FAQ.
- **General Staff:** send announcements, view Students and follow up on fees.
- **Administrators:** do all of the above, and also manage Staff, Roles, settings, branding, backups and the LINE package.

They work mostly on an office computer, sometimes on a phone. The interface is Thai first. Their jobs are recurring and file-driven: someone exports a spreadsheet from another system, uploads it, checks the preview for problems, confirms it, and then tells Students.

**Secondary: Students on LINE or Telegram.** They follow the institution's Official Account, accept the privacy notice, link their student id with an emailed one-time code, and then use the Menu and the Mini App. They are on phones, briefly, between other things. They see only their own data.

## Product Purpose

harpseal is a reusable Core for student care and student services delivered over messaging channels. It runs one Deployment per institution. It is not a multi-tenant SaaS. The Core handles the Kernel:

- identity and Link
- Consent
- roster import
- Staff, Roles and Permissions
- announcements
- quotas, audit, backups and settings

Each Deployment adds its own configuration and Modules, such as fees, grades and requests.

Success means:

- Staff move information to Students without IT help, and trust what was sent and to whom.
- Students get their own information inside the chat app they already use.

## Positioning

- **One Deployment per institution, not a shared SaaS.** Each Deployment runs on its own infrastructure: Cloudflare Workers or the institution's own server. The institution owns its data and its backups.
- **Messaging-native.** The Student surface is LINE or Telegram plus a Mini App, not another app or website to install or remember.
- **Built for spreadsheet-driven Staff.** Every import is previewed, checked row by row with blocking problems and warnings, and confirmed before anything changes. Every write is audited, and every page checks a Permission.

## Operating Context

The usual workflow:

1. Another office exports a CSV or XLSX file: the registrar's roster, finance's outstanding fees, a lecturer's scores.
2. Staff upload it and read the preview: totals, blocking problems and warnings.
3. Staff confirm the import.
4. Staff draft an announcement to the affected Students and send it.

Cadence: roster uploads happen every term. Fee files arrive per term and reminders go out before due dates. Grades come per course and exam, at different times.

Students reach everything from the chat Menu (rich menu on LINE). Pages open in the Mini App (LIFF on LINE, Web App on Telegram).

The LINE Official Account package limits how many messages can be pushed each month, so Staff watch usage.

## Capabilities and Constraints

- **Back office:** server-rendered (Rust, axum, maud, with HTMX for small interactions). There is no client build step and no JS framework. Modules add pages through the Core's registrar and must inherit the shell.
- **Workers limits:** everything must run on Cloudflare Workers (wasm32), with tight CPU and memory budgets, as well as natively.
- **Locales:** Thai is the default; English is optional per Deployment. Staff edit texts in the back office.
- **Permissions and audit:** every Staff action is gated by a Permission and audited. Destructive or broad actions, such as sending to everyone or a full roster replace, are confirmed explicitly.
- **Data limits:** announcements are at most 2,000 characters, with a small Markdown subset, up to 4 images and URL buttons. A hand-picked Audience is limited to 2,000 Students.
- **Branding:** institution name, colour, logo and rich menu images are set per Deployment in the back office, with a preview.
- **Terminology:** follow `GLOSSARY.md`. Use the Thai terms Staff see: นักศึกษา, เจ้าหน้าที่, บทบาท, สิทธิ์, กลุ่มผู้รับ, โควตา.
- **Undecided:** the exact co-branding treatment of harpseal in the back office (see Brand Commitments).

## Brand Commitments

- **Co-branded back office.** The institution's own branding leads: name, primary colour and logo from each Deployment. The back office also shows the harpseal brand, for example a "powered by harpseal" mark.
- **No harpseal assets yet.** There is no harpseal logo or wordmark in the repository, and the exact treatment is undecided. Do not invent a harpseal logo; ask before creating one.
- **Students do not see harpseal.** Student-facing surfaces (chat, Mini App) carry only the institution's brand.
- **Voice:** polite, plain Thai. Official but friendly. Short sentences. Actionable error messages that say what to do next.

## Evidence on Hand

- A fictional example Deployment, Seal Bay University (`deployments/example`), with demo seed data and default rich menu images (`deployments/example/assets/richmenu/`).
- Glossary, ADRs and the first-slice spec in the repository.
- No real customer data, testimonials or screenshots may be used in this repository. Customer names must never appear here.

## Product Principles

1. **Preview before commit.** Staff see exactly what will change, and to whom, before any import or send.
2. **Nothing silent.** Every refusal names the problem in Thai and the way to fix it. Every write is audited.
3. **The institution's product, not ours.** The institution's identity leads. harpseal stays in the background and never appears to Students.
4. **Works for the non-technical, on any device.** Readable Thai, forgiving forms, and usable on a phone.
5. **Small, durable Core.** Modules extend through stable seams, and the shell and components are shared so every page feels like one product.

## Accessibility & Inclusion

- **Thai text:** Thai typography must render correctly, with line height that suits Thai tone marks and vowels.
- **Core accessibility:** contrast, keyboard use, visible focus, labels tied to fields and errors, and `prefers-reduced-motion`.
- **Small screens:** no horizontal scroll at 360 px in the back office or the Mini App.
- **Branding contrast:** branding colours must keep contrast with text, and the back office checks this on save.
