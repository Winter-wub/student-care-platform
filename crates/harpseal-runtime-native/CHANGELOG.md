# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/Winter-wub/student-care-platform/releases/tag/harpseal-runtime-native-v0.1.0) - 2026-11-10

### Added

- *(kernel)* typed Module settings guarded by their own Permission ([#101](https://github.com/Winter-wub/student-care-platform/pull/101))
- *(kernel)* webhook handling continues when the client disconnects ([#100](https://github.com/Winter-wub/student-care-platform/pull/100))
- *(kernel)* a Module Job notifies chosen Students: chat when linked, email otherwise ([#91](https://github.com/Winter-wub/student-care-platform/pull/91))
- *(kernel)* stable Menu key for LINE rich-menu cleanup, keyed by the effective Locale ([#65](https://github.com/Winter-wub/student-care-platform/pull/65))
- *(kernel)* Mini App follow-ups: web_client Capability, Telegram Web cookie, entry fallback, Consent line, session index ([#59](https://github.com/Winter-wub/student-care-platform/pull/59))
- *(cli)* backup and restore of a Deployment through the Db and FileStore ports ([#57](https://github.com/Winter-wub/student-care-platform/pull/57))
- *(example)* demo seed, `just e2e`, CI and release-plz workflows, public-readiness ([#54](https://github.com/Winter-wub/student-care-platform/pull/54))
- *(kernel)* Mini App page, App Session for LIFF and Telegram Web App, public notice page, rate_limit table ([#51](https://github.com/Winter-wub/student-care-platform/pull/51))
- *(kernel)* usage page, Quota banner and monthly Administrator emails, failed-send counting ([#50](https://github.com/Winter-wub/student-care-platform/pull/50))
- *(announcements)* Announcements Module: create, Audience, scheduled send, acknowledge, latest-news Menu, stats, edit/resend/retract ([#48](https://github.com/Winter-wub/student-care-platform/pull/48))
- *(kernel)* admin pages: Security Events, audit log, Staff & Roles, Texts and Consent version ([#47](https://github.com/Winter-wub/student-care-platform/pull/47))
- *(kernel)* roster import: CSV/XLSX upload, preview, chunked Job, left transition, stuck-import recovery ([#45](https://github.com/Winter-wub/student-care-platform/pull/45))
- *(kernel)* send and inbound robustness: metered units per Push request, partial sends, inbound de-duplication, URL-free transport errors ([#43](https://github.com/Winter-wub/student-care-platform/pull/43))
- *(kernel)* Students pages: list, search, detail, edit with overrides, Tags, status, Manual Link ([#42](https://github.com/Winter-wub/student-care-platform/pull/42))
- *(jobs)* enqueue API, cron schedules, pruning, panic capture, /admin/jobs.json ([#41](https://github.com/Winter-wub/student-care-platform/pull/41))
- *(auth)* staff login, sessions, Roles and Permissions guard, audit writer, back-office shell, CLI bootstrap ([#38](https://github.com/Winter-wub/student-care-platform/pull/38))
- *(line)* real LINE adapter: signature, webhook events, Flex sends, rich menus, LIFF, multicast ([#34](https://github.com/Winter-wub/student-care-platform/pull/34))
- *(telegram)* real Telegram adapter: updates, inline keyboards, commands as Actions, edit/delete, Web App ([#33](https://github.com/Winter-wub/student-care-platform/pull/33))
- *(runtimes)* complete native and Cloudflare runtimes ([#32](https://github.com/Winter-wub/student-care-platform/pull/32))
- *(channel)* Fake Channel profiles, Channel contract suite, Outbound Message and usage metering ([#31](https://github.com/Winter-wub/student-care-platform/pull/31))
- *(kernel)* workspace, ports, migrations, builder validation, test harness ([#30](https://github.com/Winter-wub/student-care-platform/pull/30))
