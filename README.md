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
| `crates/harpseal-runtime-native` | Native runtime: SQLite (rusqlite), local files, OS clock and RNG, `axum::serve`. SMTP/HTTP client in #18. | native |
| `crates/harpseal-runtime-cloudflare` | Workers runtime: D1, `fetch`, `Date.now`, `crypto.getRandomValues`. R2 and Resend in #18. | wasm32 |
| `crates/harpseal-testing` | The test harness (dev-dependency): fake Services + Fake Channel + the Router, driven in-process. | native |
| `crates/harpseal-cli` | The `harpseal` command line (placeholder; #21, #29). | native |
| `deployments/example/deployment` | Seal Bay University, a fictional customer: `deployment.toml` + `builder()` / `deployment()`. | native + wasm32 |
| `deployments/example/native` | The example as a self-hosted binary. | native |
| `deployments/example/worker` | The example as a Cloudflare Worker (`wrangler.toml`). | wasm32 |

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
let router = Arc::new(d).router();          // serve with either runtime
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

Settings come from the environment: `HARPSEAL_DB` (default
`example.sqlite3`), `HARPSEAL_FILES`, `HARPSEAL_SMTP`, `HARPSEAL_ADDR`, and the
Channel secrets `LINE_CHANNEL_SECRET`, `LINE_ACCESS_TOKEN`, `LINE_LIFF_ID`,
`TELEGRAM_BOT_TOKEN`, `TELEGRAM_WEBHOOK_SECRET`, `TELEGRAM_WEB_APP_URL`,
`FAKE_SECRET` (required, non-empty: the Fake Channel refuses an empty secret). Back-office pages answer 401 until Staff
login lands (#21).

## Testing

```rust
let h = harpseal_testing::Harness::new();     // in-memory SQLite, fake email/HTTP/clock/random
let app = h.start().await;                    // build + migrate + Router
assert_eq!(app.get("/notice").await.status, 200);
app.fake_webhook([inbound]).await;            // signed Fake Channel webhook
h.fake_channel().sent_to(&account);           // what the Student saw
h.clock().advance_minutes(5);                 // time only moves when told
h.emails();  h.http_requests();               // captured, never sent
```

Use `h.builder()` to add Modules before booting, or pass any Deployment to
`h.boot(d)` (the example's tests do `example_deployment::builder(h.services(),
..).channel(h.fake_channel())`).

## Checks (CI runs the same)

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --target wasm32-unknown-unknown -p harpseal -p harpseal-channel-line \
  -p harpseal-channel-telegram -p harpseal-channel-fake -p harpseal-runtime-cloudflare
cargo doc --workspace --no-deps
```

## License

Apache-2.0 (ADR-0005).
