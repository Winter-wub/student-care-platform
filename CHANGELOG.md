# Changelog

All notable changes to the `harpseal` crate are documented here. The other
crates keep their own `CHANGELOG.md` next to their `Cargo.toml`.

This file is maintained by [release-plz](https://release-plz.dev) from the
Conventional Commit titles of merged pull requests; do not edit released
sections by hand. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/Winter-wub/student-care-platform/releases/tag/harpseal-v0.1.0) - 2026-11-10

### Added

- *(roster)* optional columns may be left out, header hints, echoed values ([#161](https://github.com/Winter-wub/student-care-platform/pull/161))
- *(texts)* one Link verb, buttons that say what they do, Tag display names ([#159](https://github.com/Winter-wub/student-care-platform/pull/159))
- *(announcements)* Audience fields by scope, year checkboxes, numeric ranges ([#160](https://github.com/Winter-wub/student-care-platform/pull/160))
- *(students)* bulk dock stays in view, phone layout, larger checkbox targets ([#158](https://github.com/Winter-wub/student-care-platform/pull/158))
- *(announcements)* saved Audiences ([#153](https://github.com/Winter-wub/student-care-platform/pull/153))
- *(students)* multi-select and bulk actions on the Students list ([#152](https://github.com/Winter-wub/student-care-platform/pull/152))
- *(picker)* Student picker for Module forms; reminders use it ([#151](https://github.com/Winter-wub/student-care-platform/pull/151))
- *(students)* Manual Link picks from recent unlinked accounts ([#148](https://github.com/Winter-wub/student-care-platform/pull/148))
- *(ui)* shorter announcement editor, upload errors with a fix, ids that do not wrap ([#150](https://github.com/Winter-wub/student-care-platform/pull/150))
- *(texts)* collapsed groups and live search ([#147](https://github.com/Winter-wub/student-care-platform/pull/147))
- *(roster)* preview shows what changes and for whom ([#134](https://github.com/Winter-wub/student-care-platform/pull/134))
- *(confirm)* review the remaining broad actions ([#135](https://github.com/Winter-wub/student-care-platform/pull/135))
- *(back-office)* polish pass ([#131](https://github.com/Winter-wub/student-care-platform/pull/131))
- *(back-office)* type scale, contrast and accessibility ([#129](https://github.com/Winter-wub/student-care-platform/pull/129))
- *(back-office)* help where the work is ([#127](https://github.com/Winter-wub/student-care-platform/pull/127))
- *(back-office)* plain Thai for Staff ([#125](https://github.com/Winter-wub/student-care-platform/pull/125))
- *(back-office)* grouped navigation and a term status home ([#123](https://github.com/Winter-wub/student-care-platform/pull/123))
- *(back-office)* proportionate confirmation for broad actions ([#120](https://github.com/Winter-wub/student-care-platform/pull/120))
- *(announcements)* headings, text colour palette and PDF attachments ([#113](https://github.com/Winter-wub/student-care-platform/pull/113))
- *(announcements)* images: upload, LINE hero, Mini App ([#109](https://github.com/Winter-wub/student-care-platform/pull/109))
- *(branding)* institution name, colour, logo and rich menu images in the back office, with preview ([#114](https://github.com/Winter-wub/student-care-platform/pull/114))
- *(usage)* the LINE OA package chosen on /admin/usage sets the message quota ([#115](https://github.com/Winter-wub/student-care-platform/pull/115))
- *(announcements)* formatted body with links, and URL buttons in chat ([#108](https://github.com/Winter-wub/student-care-platform/pull/108))
- *(auth)* Roles from [roles] are seeded once; back-office Role edits survive migrate ([#110](https://github.com/Winter-wub/student-care-platform/pull/110))
- *(kernel)* Module pages and Jobs borrow the Deployment to call Core services ([#106](https://github.com/Winter-wub/student-care-platform/pull/106))
- *(backup)* split large tables into ordered parts, format 2 ([#105](https://github.com/Winter-wub/student-care-platform/pull/105))
- *(settings)* Module Settings without a default; the kernel. namespace reserved ([#104](https://github.com/Winter-wub/student-care-platform/pull/104))
- *(roster)* integer min/max and enum value labels on attributes ([#102](https://github.com/Winter-wub/student-care-platform/pull/102))
- *(kernel)* typed Module settings guarded by their own Permission ([#101](https://github.com/Winter-wub/student-care-platform/pull/101))
- *(roster)* warnings vs blocking problems, missing-data filter on Students ([#96](https://github.com/Winter-wub/student-care-platform/pull/96))
- *(kernel)* webhook handling continues when the client disconnects ([#100](https://github.com/Winter-wub/student-care-platform/pull/100))
- *(kernel)* a Module Job notifies chosen Students: chat when linked, email otherwise ([#91](https://github.com/Winter-wub/student-care-platform/pull/91))
- *(kernel)* Module back-office spreadsheet upload with the roster's size limit ([#94](https://github.com/Winter-wub/student-care-platform/pull/94))
- *(kernel)* Mini App re-checks the platform user against the App Session ([#90](https://github.com/Winter-wub/student-care-platform/pull/90))
- *(roster)* let a Deployment turn off full mode and default to upsert ([#93](https://github.com/Winter-wub/student-care-platform/pull/93))
- *(announcements)* LINE edit notice, configurable latest count, expired items closed to Students ([#89](https://github.com/Winter-wub/student-care-platform/pull/89))
- *(kernel)* nightly backup to R2 with 30-day retention and an Admin data export to Excel ([#87](https://github.com/Winter-wub/student-care-platform/pull/87))
- *(kernel)* multi-sheet roster import on a shared, public spreadsheet reader ([#82](https://github.com/Winter-wub/student-care-platform/pull/82))
- *(kernel)* a Module's Mini App page knows its Student and links from the Mini App home ([#84](https://github.com/Winter-wub/student-care-platform/pull/84))
- *(kernel)* split the unlink Permission from Manual Link ([#85](https://github.com/Winter-wub/student-care-platform/pull/85))
- *(announcements)* hand-picked recipients for announcements ([#83](https://github.com/Winter-wub/student-care-platform/pull/83))
- *(kernel)* re-link LINE and Telegram accounts after a Menu label edit and retire the previous rich menu ([#68](https://github.com/Winter-wub/student-care-platform/pull/68))
- *(kernel)* exact 409 metering per batch key and Message index; LINE resume token from the failing request ([#67](https://github.com/Winter-wub/student-care-platform/pull/67))
- *(kernel)* stable Menu key for LINE rich-menu cleanup, keyed by the effective Locale ([#65](https://github.com/Winter-wub/student-care-platform/pull/65))
- *(roster)* import follow-ups: cached rows, no orphaned files, yearly prune, matching tallies, scoped upload limit, stricter parser ([#61](https://github.com/Winter-wub/student-care-platform/pull/61))
- *(kernel)* Mini App follow-ups: web_client Capability, Telegram Web cookie, entry fallback, Consent line, session index ([#59](https://github.com/Winter-wub/student-care-platform/pull/59))
- *(announcements)* chunked edit and retract Jobs, no send after retract, no existence leak on ack ([#58](https://github.com/Winter-wub/student-care-platform/pull/58))
- *(cli)* backup and restore of a Deployment through the Db and FileStore ports ([#57](https://github.com/Winter-wub/student-care-platform/pull/57))
- *(example)* demo seed, `just e2e`, CI and release-plz workflows, public-readiness ([#54](https://github.com/Winter-wub/student-care-platform/pull/54))
- *(kernel)* Mini App page, App Session for LIFF and Telegram Web App, public notice page, rate_limit table ([#51](https://github.com/Winter-wub/student-care-platform/pull/51))
- *(kernel)* usage page, Quota banner and monthly Administrator emails, failed-send counting ([#50](https://github.com/Winter-wub/student-care-platform/pull/50))
- *(announcements)* Announcements Module: create, Audience, scheduled send, acknowledge, latest-news Menu, stats, edit/resend/retract ([#48](https://github.com/Winter-wub/student-care-platform/pull/48))
- *(kernel)* admin pages: Security Events, audit log, Staff & Roles, Texts and Consent version ([#47](https://github.com/Winter-wub/student-care-platform/pull/47))
- *(kernel)* roster import: CSV/XLSX upload, preview, chunked Job, left transition, stuck-import recovery ([#45](https://github.com/Winter-wub/student-care-platform/pull/45))
- *(kernel)* send and inbound robustness: metered units per Push request, partial sends, inbound de-duplication, URL-free transport errors ([#43](https://github.com/Winter-wub/student-care-platform/pull/43))
- *(kernel)* Students pages: list, search, detail, edit with overrides, Tags, status, Manual Link ([#42](https://github.com/Winter-wub/student-care-platform/pull/42))
- *(kernel)* Link lifecycle: Staff revoke, Manual Link, unreachable skip, re-consent across Channels ([#40](https://github.com/Winter-wub/student-care-platform/pull/40))
- *(jobs)* enqueue API, cron schedules, pruning, panic capture, /admin/jobs.json ([#41](https://github.com/Winter-wub/student-care-platform/pull/41))
- *(auth)* staff login, sessions, Roles and Permissions guard, audit writer, back-office shell, CLI bootstrap ([#38](https://github.com/Winter-wub/student-care-platform/pull/38))
- *(kernel)* chat flow: welcome, Consent, student id, email OTP, Link, Menu unlock ([#37](https://github.com/Winter-wub/student-care-platform/pull/37))
- *(line)* real LINE adapter: signature, webhook events, Flex sends, rich menus, LIFF, multicast ([#34](https://github.com/Winter-wub/student-care-platform/pull/34))
- *(telegram)* real Telegram adapter: updates, inline keyboards, commands as Actions, edit/delete, Web App ([#33](https://github.com/Winter-wub/student-care-platform/pull/33))
- *(runtimes)* complete native and Cloudflare runtimes ([#32](https://github.com/Winter-wub/student-care-platform/pull/32))
- *(channel)* Fake Channel profiles, Channel contract suite, Outbound Message and usage metering ([#31](https://github.com/Winter-wub/student-care-platform/pull/31))
- *(kernel)* workspace, ports, migrations, builder validation, test harness ([#30](https://github.com/Winter-wub/student-care-platform/pull/30))

### Fixed

- *(admin)* Thai tap-area labels on branding, grouped quota numbers ([#149](https://github.com/Winter-wub/student-care-platform/pull/149))
- *(channels)* [**breaking**] refuse dev-only Channels outside dev mode ([#133](https://github.com/Winter-wub/student-care-platform/pull/133))
- *(kernel)* inbound/send hardening: lost-claim recovery, 409 metering, Inbound constructors, Cloudflare error scrubbing, D1 changes check ([#60](https://github.com/Winter-wub/student-care-platform/pull/60))
- *(kernel)* keep the OTP attempt claim the only gate under racing answers ([#39](https://github.com/Winter-wub/student-care-platform/pull/39))

### Other

- *(example)* rename the DDS/DDS-Intl programme values to MSC/MSC-Intl ([#56](https://github.com/Winter-wub/student-care-platform/pull/56))
