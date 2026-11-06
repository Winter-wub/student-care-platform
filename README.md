# harpseal

A reusable Rust Core for student care systems that talk to students over
messaging channels (LINE, Telegram). Each customer runs their own
Deployment: this Core plus their configuration and Modules. Vocabulary is in
[`GLOSSARY.md`](GLOSSARY.md); decisions are in [`docs/adr/`](docs/adr/); the
first slice is specified in [`docs/specs/first-slice.md`](docs/specs/first-slice.md).

## Crates

| Crate | What it is | Targets |
|---|---|---|
| `crates/harpseal` | The Core: message model, `Channel` and `Module` traits, the six runtime ports (`Db`, `FileStore`, `EmailSender`, `HttpClient`, `Clock`, `Random`) in `Services`, `Deployment::builder()` with build-time validation, Kernel migrations, the axum `Router`, the shared page layout. | native + wasm32 |
| `crates/harpseal-channel-line` | LINE adapter: renderer and request packing (real), HTTP calls (#16). | native + wasm32 |
| `crates/harpseal-channel-telegram` | Telegram adapter: renderer and call building (real), HTTP calls (#17). | native + wasm32 |
| `crates/harpseal-channel-fake` | Fake Channel: in-memory, records what the Student saw; used by tests and demos. | native + wasm32 |
| `crates/harpseal-runtime-native` | Native runtime: SQLite (rusqlite), local directory or S3-compatible files (SigV4), SMTP (lettre) or Resend email, reqwest, OS clock and RNG, `axum::serve`, a per-minute ticker. | native |
| `crates/harpseal-runtime-cloudflare` | Workers runtime: D1, R2, Resend over `fetch`, `Date.now`, `crypto.getRandomValues`, panic hook, and the `fetch` / `scheduled` (Cron) entry points. | wasm32 |
| `crates/harpseal-testing` | The test harness (dev-dependency): fake Services + Fake Channel + the Router, driven in-process. | native |
| `crates/harpseal-cli` | The `harpseal` command line (placeholder; #21, #29). | native |
| `deployments/example/deployment` | Seal Bay University, a fictional customer: `deployment.toml` + `builder()` / `deployment()`. | native + wasm32 |
| `deployments/example/native` | The example as a self-hosted binary (`harpseal-example-native`). | native |
| `deployments/example/worker` | The example as a Cloudflare Worker (`harpseal-example-worker`, `wrangler.toml`). | wasm32 |

The Core and the channel crates have no platform feature flags; everything
platform-specific lives in a runtime crate. `harpseal`'s `fixtures` feature
only exposes sample messages to adapter tests.

## Public API rules

- Everything a Deployment or Module author needs is re-exported from the
  crate root (`harpseal::Deployment`, `harpseal::Message`, ...). Free
  functions are grouped in `harpseal::render`, `harpseal::ui` and
  `harpseal::password`. Everything else is private.
- Public enums and config structs are `#[non_exhaustive]`; config structs
  come from TOML (`DeploymentConfig::from_toml`) or a `new` constructor.
- Every library crate has `#![warn(missing_docs)]` and `#![forbid(unsafe_code)]`.
- The Kernel's schema (`crates/harpseal/migrations/*.sql`) is not public API.

## Assembling a Deployment

```rust
let d = Deployment::builder()
    .config(DeploymentConfig::from_toml(TOML)?)
    .runtime(services)                       // from a runtime crate
    .channel(LineChannel::new(line, services.http.clone()))
    .channel(FakeChannel::new(secret))
    .verification(EmailOtp::new(config.otp.clone()))
    .module(MyModule)
    .build()?;                               // BuildError on any misconfiguration
d.migrate().await?;                          // Kernel first, then Modules; idempotent
let d = Arc::new(d);
let router = d.clone().router();             // serve with either runtime
d.tick(now).await?;                          // once a minute: claims and runs due Jobs
```

`build()` refuses: a missing config, runtime, Verification Method or Channel;
two Channels of one kind; two handlers for one Action id or Job kind; a
Permission declared twice; a Role naming an undeclared Permission; a
back-office route guarded by an undeclared Permission; a Module that sends
Mini App buttons when a Channel has no Mini App URL; a Menu label key with no
text in the default Locale.

Storage follows ADR-0007: `Services::new_id()` makes UUIDv7 text ids from the
Clock and Random ports, `Services::now_ms()` gives integer milliseconds UTC,
and `audit_log` (plus `security_event` and `consent` deletes) are refused by
triggers.

## Running the example natively

```sh
FAKE_SECRET=change-me cargo run -p harpseal-example-native
# migrations: 3 applied, 0 already done
# listening on http://127.0.0.1:8080  (try /notice)
```

Every setting comes from the environment (`NativeSettings::from_env`);
secrets never go in TOML. Empty values count as unset.

| Variable | Default | Meaning |
|---|---|---|
| `HARPSEAL_DB` | `harpseal.sqlite3` | SQLite file (created and migrated at start) |
| `HARPSEAL_ADDR` | `127.0.0.1:8080` | listen address |
| `HARPSEAL_FILES` | `files` | FileStore directory |
| `HARPSEAL_S3_BUCKET` | unset | use an S3-compatible bucket instead of `HARPSEAL_FILES` |
| `HARPSEAL_S3_ENDPOINT` | required with a bucket | `https://<account>.r2.cloudflarestorage.com`, `https://s3.<region>.amazonaws.com`, MinIO, ... (path-style) |
| `HARPSEAL_S3_REGION` | `auto` | SigV4 signing region |
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | required with a bucket | S3 credentials |
| `HARPSEAL_EMAIL` | `smtp` | `smtp` or `resend` |
| `HARPSEAL_SMTP` | `smtp://localhost:25` | lettre URL: `smtp://host:25`, `smtp://user:pass@host:587?tls=required`, `smtps://user:pass@host:465` |
| `RESEND_API_KEY` | required with `resend` | Resend API key |
| `HARPSEAL_EMAIL_FROM` | `noreply@localhost` | sender address (`Name <addr@domain>`) |

The Deployment's secrets: `LINE_CHANNEL_SECRET`, `LINE_ACCESS_TOKEN`,
`LINE_LIFF_ID`, `TELEGRAM_BOT_TOKEN`, `TELEGRAM_WEBHOOK_SECRET`,
`TELEGRAM_WEB_APP_URL`, `FAKE_SECRET` (required, non-empty: the Fake Channel
refuses an empty secret). A timer calls `Deployment::tick` every minute.
Back-office pages answer 401, and `/admin/login` 404, until Staff login lands
(#21).

## Deploying the example to Workers

Workers Paid is required (Free's 10 ms CPU fails logins and imports). The
build needs `worker-build` (`cargo install worker-build`) and binaryen's
`wasm-opt` (`brew install binaryen`); `wrangler.toml`'s `[build]` command runs
`worker-build --release --no-opt`, then `wasm-opt -Oz`.

Locally, with no Cloudflare account (D1 is simulated by miniflare):

```sh
cd deployments/example/worker
echo 'FAKE_SECRET=change-me' > .dev.vars        # git-ignored
npx wrangler@4 dev --local                       # http://localhost:8787/notice
curl 'http://localhost:8787/__scheduled?cron=*+*+*+*+*'   # with --test-scheduled: runs one tick
```

The first request in an isolate builds the Deployment and applies its
migrations to D1; later requests reuse it. A Cron Trigger (`* * * * *`, UTC)
calls `Deployment::tick` at the trigger's scheduled time.

A preview on your own account:

```sh
cd deployments/example/worker
npx wrangler@4 d1 create harpseal-example            # paste database_id into wrangler.toml
npx wrangler@4 secret put FAKE_SECRET                 # required
npx wrangler@4 secret put RESEND_API_KEY              # optional: without it every email send fails
npx wrangler@4 secret put LINE_CHANNEL_SECRET         # and LINE_ACCESS_TOKEN, LINE_LIFF_ID,
                                                      # TELEGRAM_BOT_TOKEN, TELEGRAM_WEBHOOK_SECRET,
                                                      # TELEGRAM_WEB_APP_URL when the Channels are set up
npx wrangler@4 r2 bucket create harpseal-example-files   # optional: then uncomment [[r2_buckets]]
npx wrangler@4 deploy
curl https://harpseal-example.<subdomain>.workers.dev/notice
```

Bindings the runtime reads (`CloudflareSettings`): D1 `DB`, R2 `FILES`
(optional: without it file operations answer an error naming the binding),
secret `RESEND_API_KEY`, var `EMAIL_FROM` (set in `wrangler.toml`).
`Date.now()` is frozen while a request computes, so never measure durations
with the Clock there; use `wrangler tail`.

## Testing

```rust
let h = harpseal_testing::Harness::new();     // in-memory SQLite, fake email/HTTP/clock/random
let app = h.start().await;                    // build + migrate + Router
assert_eq!(app.get("/notice").await.status, 200);
app.fake_webhook([inbound]).await;            // signed Fake Channel webhook (text, file, Action, follow, blocked, unblocked)
h.fake_channel().seen_by(&account);           // what the Student saw, with Delivery and Metering
app.count("SELECT count(*) FROM outbound_message").await;  // rows a later request reads back
h.clock().advance_minutes(5);                 // time only moves when told
h.emails();  h.http_requests();               // captured, never sent
```

`Harness::new().with_fake_profile(FakeProfile::LineLike)` makes the Fake
Channel imitate LINE (one reply token covers 5 objects, Push metered, LINE's
degrade limits, no attachments); `FakeProfile::TelegramLike` has no reply
tokens, meters nothing and sends one message per Card.

The Channel contract suite lives in `harpseal_testing::contract`: an adapter
crate adds `harpseal-testing` as a dev-dependency and calls
`contract::run(|| target)` with its Channel, recorded webhooks (`Fixtures`) and
a `Wire` reading its `RecordingHttp`. It runs green against the Fake Channel in
every profile; the LINE and Telegram runs are `#[ignore]` until #16/#17.

Runtime port tests live in `crates/harpseal-runtime-native/tests` (SQLite
batch rollback, local files, a mock SMTP server, Resend and S3 signing against
the recording HttpClient, the ticker). The real-S3 round trip runs only when
`HARPSEAL_TEST_S3_BUCKET`, `HARPSEAL_TEST_S3_ENDPOINT`, `AWS_ACCESS_KEY_ID` and
`AWS_SECRET_ACCESS_KEY` are set.

Use `h.builder()` to add Modules before booting, or pass any Deployment to
`h.boot(d)` (the example's tests do `example_deployment::builder(h.services(),
..).channel(h.fake_channel())`).

## Checks (CI runs the same)

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --target wasm32-unknown-unknown -p harpseal -p harpseal-channel-line \
  -p harpseal-channel-telegram -p harpseal-channel-fake -p harpseal-runtime-cloudflare \
  -p harpseal-example-worker
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

## License

Apache-2.0 (ADR-0005).
