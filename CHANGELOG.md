# Changelog

All notable changes to the `harpseal` crate are documented here. The other
crates keep their own `CHANGELOG.md` next to their `Cargo.toml`.

This file is maintained by [release-plz](https://release-plz.dev) from the
Conventional Commit titles of merged pull requests; do not edit released
sections by hand. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Module pages and Jobs can call the Core's code-level services (#106):
  `PageState::deployment()` and `JobContext::deployment()` borrow the
  `Deployment` (never keep it beyond the request or run), so a back-office
  handler calls `modules::announcements::create_draft` / `confirm` as
  `Actor::staff(&staff)` with its `RequestCtx`, checked against
  `announcements.send` and audited as that Staff member.
  `ui::service_error` answers a refused call with the back-office guard's
  403 page (any other error: a 500 page). The example's `/admin/reminders`
  drafts an announcement to the pasted Students.
