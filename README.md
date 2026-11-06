# harpseal

A reusable Rust Core for student care systems that talk to students over
messaging channels (LINE, Telegram). It is not a multi-tenant SaaS: each
customer runs their own Deployment, this Core plus their configuration
and Modules, on Cloudflare Workers or on their own server.

What the first slice does, end to end:

- **Students** follow the bot, accept the privacy notice (Consent), type
  their student id, confirm the code emailed to them (OTP) and are linked.
  The Menu then unlocks: latest news, the customer's own entries, unlink.
- **Staff** sign in to a server-rendered back office: import the roster
  (CSV or XLSX), browse and edit Students and Links, send announcements to an
  Audience and see who acknowledged them, watch Quotas, Security Events and
  the audit log, manage Staff, Roles and texts.
- **Every write is audited** and every Staff page checks a Permission.

Vocabulary is in [`GLOSSARY.md`](https://github.com/Winter-wub/student-care-platform/blob/main/GLOSSARY.md), decisions in
[`docs/adr/`](https://github.com/Winter-wub/student-care-platform/tree/main/docs/adr/), the first slice's spec in
[`docs/specs/first-slice.md`](https://github.com/Winter-wub/student-care-platform/blob/main/docs/specs/first-slice.md).

## Crates

| Crate | What it is | Targets |
|---|---|---|
| [`harpseal`](https://github.com/Winter-wub/student-care-platform/tree/main/crates/harpseal) | The Core: message model, `Channel` and `Module` traits, the six runtime ports (`Db`, `FileStore`, `EmailSender`, `HttpClient`, `Clock`, `Random`) in `Services`, `Deployment::builder()` with build-time validation, the Kernel (identity flow, roster, Students, Staff auth, admin pages, Jobs, usage, Mini App), the Announcements Module, the axum `Router`. | native + wasm32 |
| [`harpseal-channel-line`](https://github.com/Winter-wub/student-care-platform/tree/main/crates/harpseal-channel-line) | LINE adapter: signed webhooks, Flex rendering with degrade rules, Reply/Push/multicast, rich menus, LIFF. | native + wasm32 |
| [`harpseal-channel-telegram`](https://github.com/Winter-wub/student-care-platform/tree/main/crates/harpseal-channel-telegram) | Telegram adapter: secret-token webhook, Updates → Inbound, commands as Actions, `setMyCommands` Menus, edit/delete, Web App `initData`. | native + wasm32 |
| [`harpseal-channel-fake`](https://github.com/Winter-wub/student-care-platform/tree/main/crates/harpseal-channel-fake) | Fake Channel: in-memory, records what the Student saw; for tests, demos and `just e2e`. | native + wasm32 |
| [`harpseal-runtime-native`](https://github.com/Winter-wub/student-care-platform/tree/main/crates/harpseal-runtime-native) | Native runtime: SQLite, local or S3-compatible files, SMTP or Resend, `axum::serve`, a per-minute ticker. | native |
| [`harpseal-runtime-cloudflare`](https://github.com/Winter-wub/student-care-platform/tree/main/crates/harpseal-runtime-cloudflare) | Workers runtime: D1, R2, Resend over `fetch`, Cron, the `fetch` / `scheduled` entry points. | wasm32 |
| [`harpseal-cli`](https://github.com/Winter-wub/student-care-platform/tree/main/crates/harpseal-cli) | The `harpseal` command line as a library each Deployment's binary calls: `migrate`, `staff create`. | native |
| [`harpseal-testing`](https://github.com/Winter-wub/student-care-platform/tree/main/crates/harpseal-testing) | Test harness (dev-dependency): fake Services, the Fake Channel and the Router in-process, a cookie-keeping `Browser`, the Channel contract suite. | native |

The example, never published:

| Package | What it is |
|---|---|
| `deployments/example/deployment` | Seal Bay University (มหาวิทยาลัยอ่าวแมวน้ำ), a fictional customer: `deployment.toml`, a small customer Module, `builder()`, the demo seed. |
| `deployments/example/data` | The example roster: 50 fictional Students as `roster.csv` and `roster.xlsx`. |
| `deployments/example/native` | The example as a self-hosted binary (`harpseal-example-native`) and its `harpseal` CLI. |
| `deployments/example/worker` | The example as a Cloudflare Worker (`harpseal-example-worker`, `wrangler.toml`). |
| `deployments/example/e2e` | `just e2e`: boots the native binary and walks the first slice over HTTP. |

The Core and the channel crates have no platform feature flags; everything
platform-specific lives in a runtime crate. Public API rules: everything a
Deployment or Module author needs is re-exported from each crate root,
public enums and config structs are `#[non_exhaustive]`, every library has
`#![warn(missing_docs)]` and `#![forbid(unsafe_code)]`, and the Kernel's
schema is not public API.

## Quick start: native

Needs Rust (the toolchain in `rust-toolchain.toml` installs itself), a C
compiler (rustls' `aws-lc-sys`) and [`just`](https://github.com/casey/just).

```sh
just seed                         # migrate, first Administrator, demo data
# created staff admin@sealbay.example (01a1…)
# temporary password (shown once, change it at first sign-in): 839c9e-c8b47a-1015c8-ad0fe8
# seeded as admin@sealbay.example: roster import 01a1… (50 Students), 4 announcements created, 2 texts written
FAKE_SECRET=$(openssl rand -hex 16) just run   # http://127.0.0.1:8080/notice
```

Sign in at <http://127.0.0.1:8080/admin/login> with the temporary password
and choose a new one. `just run` starts the example in dev mode: email is
captured instead of sent, and `/dev/outbox`, `/dev/email` and `/dev/tick`
(header `x-dev-secret: <FAKE_SECRET>`) show what a Student would see on
their phone. Dev mode refuses to start on a non-loopback address or with a
`FAKE_SECRET` shorter than 16 characters or equal to the `change-me`
placeholder. Without `just`:

```sh
export FAKE_SECRET=$(openssl rand -hex 16) LINE_LIFF_ID=0000000000-devDummy \
       TELEGRAM_WEB_APP_URL=https://dev.sealbay.invalid/tg
cargo run -p harpseal-example-native --bin harpseal -- staff create --admin --email admin@sealbay.example
cargo run -p harpseal-example-native --bin harpseal -- seed-example
HARPSEAL_DEV_ROUTES=1 cargo run -p harpseal-example-native
```

All settings come from the environment (see
[`deployments/example/native/.env.example`](https://github.com/Winter-wub/student-care-platform/blob/main/deployments/example/native/.env.example)
and [self-hosting](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/self-hosting.md)).

## Quick start: Cloudflare Workers

Workers Paid is required. Locally, with no Cloudflare account:

```sh
cargo install worker-build && brew install binaryen
cd deployments/example/worker
cp .dev.vars.example .dev.vars    # placeholders; git-ignored
npx wrangler@4 dev --local        # http://localhost:8787/notice
```

Creating the D1 database, secrets and the deploy are in
[Deploying to Workers](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/workers.md).

## `just` recipes

| Recipe | What it does | Network |
|---|---|---|
| `just check` (= `just ci`) | The definition-of-done checks, as CI runs them: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo check --target wasm32-unknown-unknown` of the six wasm crates, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | no |
| `just fmt` | `cargo fmt --all` | no |
| `just run` | The example on `127.0.0.1:8080` in dev mode (needs `FAKE_SECRET`) | no |
| `just seed [--force]` | `migrate`, `staff create --admin`, `seed-example` against `HARPSEAL_DB` | no |
| `just e2e` | Builds the example, boots it on a free port with a temporary database and walks the first slice over HTTP; prints a pass/fail table | no |
| `just release-dry` | `release-plz update` in a throwaway worktree (the versions and changelogs the next Release PR would carry), then `cargo publish --workspace --dry-run`; publishes nothing | yes |

Every recipe echoes its commands. `cargo` downloads crates on the first
build.

## Hosting your own Deployment

A customer Deployment is a repository of its own (ADR-0001) shaped like
`deployments/example`:

1. A library crate with `deployment.toml` (name, Locale, `[student]`,
   `[roster]` mapping, `[roles]`, `[otp]`, `[quotas]`, `[texts.<locale>]`)
   and a `builder()` that adds the Channels, `EmailOtp`, the Announcements
   Module and the customer's own Modules
   ([Building a Deployment](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/deployment.md)).
2. A runtime binary: `harpseal-runtime-native` on a server of your own
   ([self-hosting](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/self-hosting.md)) or
   `harpseal-runtime-cloudflare` on Workers ([Workers](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/workers.md)),
   plus a `harpseal` CLI binary calling `harpseal_cli::run`.
3. The Channels: a LINE Official Account with a LIFF app, a Telegram bot
   with a Web App URL ([Channels](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/channels.md)), and an email
   sender (Resend, or SMTP natively).
4. Tests that boot it on `harpseal-testing`
   ([Testing](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/testing.md)).

Depend on the published crates (a `git` or `path` dependency while
developing) and pin a released version before handing over. Secrets live
only in the environment or Worker secrets, never in TOML.

## Documentation

- [Building a Deployment](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/deployment.md): the builder, what
  `build()` refuses, Jobs, sends, retries, webhook duplicates, writing from code
- [Self-hosting](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/self-hosting.md): settings, the CLI, the demo
  seed, dev mode, reverse proxies
- [The back office](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/back-office.md): every page and its Permission
- [Channels](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/channels.md): LINE, Telegram, the Mini App
- [Deploying to Workers](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/workers.md)
- [Testing](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/testing.md): the harness, the contract suite, `just e2e`
- [Releasing](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/releasing.md): release-plz, the Release PR, crates.io and the owner's setup
- [`GLOSSARY.md`](https://github.com/Winter-wub/student-care-platform/blob/main/GLOSSARY.md), [`docs/adr/`](https://github.com/Winter-wub/student-care-platform/tree/main/docs/adr/),
  [`docs/specs/first-slice.md`](https://github.com/Winter-wub/student-care-platform/blob/main/docs/specs/first-slice.md)

## Contributing, security, releases

See [`CONTRIBUTING.md`](https://github.com/Winter-wub/student-care-platform/blob/main/CONTRIBUTING.md) (one pull request per ticket,
Conventional Commits, `just check`) and [`SECURITY.md`](https://github.com/Winter-wub/student-care-platform/blob/main/SECURITY.md) (report
vulnerabilities privately). Releases are cut by
[release-plz](https://release-plz.dev): Release PRs from Conventional
Commits; only merging a Release PR tags `<crate>-v<version>` and publishes
(see [Releasing](https://github.com/Winter-wub/student-care-platform/blob/main/docs/guide/releasing.md)). Changelogs: [`CHANGELOG.md`](https://github.com/Winter-wub/student-care-platform/blob/main/CHANGELOG.md)
(the Core) and each crate's own `CHANGELOG.md`.

## License

[Apache-2.0](https://github.com/Winter-wub/student-care-platform/blob/main/LICENSE) (ADR-0005). Copyright 2026 harpseal contributors.
