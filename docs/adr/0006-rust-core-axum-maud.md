---
status: accepted
date: 2026-11-05
supersedes: ADR-0002, ADR-0004
---

# The Core is written in Rust: one library, axum, maud, a thin `Db` trait

The owner chose Rust over TypeScript for the Core on 2026-11-05, keeping the intent of ADR-0002 (Cloudflare-first, self-hostable) and ADR-0004 (server-rendered back office) but changing the language. The Core is a platform-neutral Rust library. Two thin runtime crates wrap it: `runtime-cloudflare` compiles it to wasm via workers-rs (D1, R2, Cron Triggers, HTTP email API) and `runtime-native` is a single tokio binary for a customer's own server (SQLite file, S3-compatible storage, timer, SMTP). HTTP is axum in both places (workers-rs's `http` layer). HTML is maud plus HTMX. SQL is written once in the SQLite dialect behind a small `Db` trait with one implementation per runtime; migrations are `.sql` files per crate; no ORM. Modules and customer code are crates implementing a `Module` trait, statically linked into the customer's binary; data-only configuration (initial Roles, Office Hours, Quotas, bot text) lives in TOML beside the binary. Crates are published to crates.io under the `harpseal-` prefix (no scopes there).

We rejected staying on TypeScript (owner's call), Workers-only (loses self-hosting), native-only on Cloudflare Containers (no free tier, D1/R2 over HTTP), ORMs (none span D1 and a local SQLite), and runtime-loaded plugins (wasm has no stable plugin story; static linking is simpler to ship and audit).

## Consequences

- ADR-0002's constraints still hold: no Cloudflare API outside `runtime-cloudflare`, SQLite-only SQL.
- Everything in the Core must compile to `wasm32-unknown-unknown`; crates that need threads, sockets or the filesystem belong in `runtime-native`.
- Orca workers for Rust tickets run Opus 5.5 from the start (owner's instruction), not Sonnet 5.5.
- ADR-0003 (semantic message model) and ADR-0005 (open source) are unchanged; "npm public registry" in ADR-0005 now reads crates.io.
