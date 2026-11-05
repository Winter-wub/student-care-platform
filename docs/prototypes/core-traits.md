# Prototype: Module, Deployment builder, Channel (issue #4)

Throwaway. Two public APIs over one shared vocabulary, both compiled in `crates/harpseal`
(`design_a`, `design_b`), both used by `deployments/example/deployment/src/lib.rs`, the same fictional
Deployment (Seal Bay University) written once per design. Platform calls are `todo!()`.
Checks: `cargo check --workspace` (native) and `cargo check --target wasm32-unknown-unknown -p harpseal
-p harpseal-channel-line -p harpseal-channel-telegram -p harpseal-channel-fake` both pass on rustc 1.99.1.

## Shared by both designs (crate root)

- **Message model (ADR-0003)** `message`: `Message::{Text, Card, CardList, Choices, Attachment}`, `Button::{Action, MiniApp}`,
  `Menu`/`MenuEntry`, `Locale`. `ActionId::new` rejects empty or > 64 bytes (Telegram `callback_data`); it is
  the only constructor, so an over-long id cannot reach an adapter.
- **Delivery** `delivery`: `Inbound::{Text, File, Action, Blocked}` (every variant carries `ChannelAccountId` and an
  optional `ReplyToken`); `Delivery::select(token, now)` is the only place Reply vs Push is chosen (Reply iff
  an unexpired token). Adapters return `SendReceipt { message_id, metering: Metered | Free }`; the Core writes
  an `OutboundMessage` row for every send. Metering is reported by the adapter, not inferred from Reply/Push
  (Telegram is always free, LINE Push is metered). `Capabilities { edit_message, delete_message, mini_app }`.
- **Db values** `port_types`: `Statement { sql, params: Vec<Value> }` (SQLite dialect, `?` params), `Rows`,
  `ExecResult`, `Value::{Null, Integer, Real, Text, Blob}`. Integers beyond ±2^53 are unsafe on D1: use text ids.
- **Manifest** `manifest`: `Permission`, `MenuEntrySpec`, `CatalogEntry`, `JobSpec`, `Job`, `Migration`, `Catalog`
  (`t(locale, key)` falls back to the default Locale, then to the key).
- **Config** `config::DeploymentConfig::from_toml`: `name`, `default_locale`, `roles`, `office_hours`, `quotas`, `texts`.
  `texts` override Module defaults.
- **Errors**: `Error::{Unsupported(Capability), Unreachable, RateLimited, BadSignature, Invalid, Db, Platform}`;
  `BuildError::{MissingConfig, MissingRuntime, MissingVerification, NoChannel, DuplicateChannel, DuplicateAction,
  DuplicatePermission, UnknownPermission, UnguardedRoute}`.
- **Send everywhere**: every port and every future is `Send` so axum handlers work. On Workers the JS-backed
  futures are `!Send`; `runtime-cloudflare` wraps them (`worker::send`), safe on a single-threaded isolate.

## Design A: trait objects + runtime registry (`harpseal::design_a`)

- **Ports**: `Db { query, execute, batch }` (no `begin`; `batch` is all-or-nothing), `FileStore { put, get, delete }`,
  `EmailSender { send }`, `HttpClient { send(http::Request<Vec<u8>>) }`, `Clock { now }`, `Random { fill }`. All
  `#[async_trait]`, object-safe, held as `Arc<dyn _>` in `Services` (cloneable struct of six fields).
- **Plugging a runtime**: the runtime crate returns a `Services` (`harpseal_runtime_native::services(&settings)`);
  the Core never names a concrete runtime type.
- **Channel**: `kind() -> ChannelKind`, `parse_webhook(WebhookRequest) -> Vec<Inbound>` (must return
  `BadSignature` before parsing), `download(&IncomingFile) -> bytes`, `send(to, &Delivery, &Message) -> SendReceipt`,
  `set_menu(to, Option<&Menu>)` (`None` = locked pre-Link Menu). Capabilities are accessors defaulting to `None`:
  `editor() -> Option<&dyn EditMessage>`, `deleter()`, `mini_app() -> Option<&dyn MiniApp>` (`launch_url`,
  `verify_session(token) -> ChannelAccountId` for App Session). `capabilities()` is derived from them, so the
  declaration and the behaviour cannot disagree. The Core queries with `if let Some(e) = channel.editor()`
  (`Deployment::update_message` edits or re-sends).
- **Module**: `name() -> &'static str` and `register(&self, &mut ModuleRegistrar)`. Registrar methods (chainable):
  `permission`, `menu_entry(spec, handler)`, `action(id, handler)`, `back_office(permission_id, Router<AppState>)`,
  `messages(&[CatalogEntry])`, `job(spec, handler)`, `migration(name, include_str!(..))`. Handlers are any
  `Fn(ActionContext) -> impl Future<Output = Result<Vec<Message>>> + Send` (or `JobHandler` for Jobs).
  `ActionContext { services, catalog, student, account, locale }` with `t(key)`.
- **Deployment**: `Deployment::builder().config(..).runtime(services).channel(..)*.verification(..).module(..)*.build()`
  → `Result<Deployment, BuildError>`. Everything is checked in `build()`: missing parts, duplicate channel kinds
  or Action ids, Role naming an undefined Permission, back-office route guarded by an undefined Permission.
  Then `Arc<Deployment>::router()` (webhooks at `/webhook/{kind}`, Module pages under `/admin`), `send`,
  `update_message`, `run_job`, `migrations()`, `menu_for(locale)`.
- **Verification**: `VerificationMethod { id, start(services, claim) -> Challenge, check(..) -> Option<StudentId> }`,
  implemented by `EmailOtp`.

## Design B: generics + static dispatch (`harpseal::design_b`)

- **Ports**: same six traits and methods, written as `fn .. -> impl Future<Output = ..> + Send` (native RPITIT, no
  boxing, not `dyn`-compatible). `trait Runtime { type Db; type FileStore; type Email; type Http; type Clock;
  type Random; fn db(&self) -> &Self::Db; .. }`.
- **Plugging a runtime**: the runtime crate defines one type (`NativeRuntime`) implementing `Runtime`, passed to
  `Deployment::builder(runtime)`. The runtime is a constructor argument, so it cannot be missing.
- **Channel**: same four required methods plus `const KIND: ChannelKind` and three associated types
  `Editor`, `Deleter`, `MiniApp`. An adapter without a Capability writes `type Editor = Unsupported;` (an
  uninhabited enum, so `Option<&Unsupported>` is always `None`). Stable Rust has no associated-type defaults:
  **every adapter names all three**, and adding a Capability edits every adapter. Channels live in a type list
  `ChannelSet` (`()` or `(Head, Tail)`) which dispatches by comparing `KIND`; each Kernel operation that reaches
  a Channel needs a `ChannelSet` method and two impls.
- **Module**: `trait Module<R: Runtime>` with consts `NAME`, `PERMISSIONS`, `CATALOG`, `JOBS`, `MIGRATIONS`
  (defaults `&[]`) and methods `menu()`, `back_office() -> Option<(permission, BackOffice<R>)>`,
  `on_action(&ActionCx<R>, &ActionId) -> Option<Vec<Message>>` (`None` = not mine), `run_job(..) -> bool`.
  Behaviour is a `match` inside the Module. A Module is generic over the runtime: back-office handlers are
  written `hello_page::<R>`.
- **Deployment**: typestate builder `Deployment::builder(rt).channel(..)*.verification(..).module(..)*.build(config)`.
  Each call grows the type (`Builder<R, (Fake, (Telegram<H>, (Line<H>, ()))), (Hello, ()), EmailOtp>`).
  `build()` exists only once a Verification Method is set: forgetting it is a compile error. Duplicates and
  Permission checks are still run-time `BuildError`s (consts cannot be compared across Modules). A function
  that returns a Deployment must spell its type (the example uses a `type Example<R, H> = ..` alias).
- **Channel adapters** that call the platform are generic over the runtime's HTTP type (`LineChannel<H>`), so
  the customer passes an `H: HttpClient + Clone` value besides the runtime.

## Comparison

| | Design A (dyn + registry) | Design B (generics) |
|---|---|---|
| Depth | Module = 2 methods; the registrar hides merging, validation, dispatch. Channel = 5 required + 3 optional accessors. | Module = 5 consts + 4 methods (all defaulted). Channel adds 3 mandatory assoc types; Core grows `ChannelSet`/`ModuleSet` plumbing per operation. Shallower. |
| Locality of change | New Capability: one default accessor, no adapter touched. New Kernel operation over Channels: one call. New port: one `Services` field. | New Capability: every adapter edits (no assoc-type defaults). New Kernel operation: new `ChannelSet` method + 2 impls. New port: one assoc type in every runtime. |
| Customer code | Imperative chain with closures; return type `Deployment`. Reads like a config file. | Consts + `match` on Action id; `impl<R: Runtime>` and `::<R>` everywhere; full type alias needed to return the Deployment. |
| Wasm impact (reasoned, not measured) | One copy of Kernel code; vtables keep every trait method alive; one heap allocation per async call (`async_trait`). Negligible next to D1/HTTP latency. | One runtime per binary, so monomorphisation makes one copy anyway; unused paths (`Unsupported`) are removed; no per-call boxing. Likely a few KB smaller; same order. |
| Compile-time errors | Missing parts and duplicates surface at `build()` (startup / first test). `Send` errors in handlers: 22 lines, point at the closure. | Missing Verification is a compile error ("method `build` exists … `NoVerification: VerificationMethod<R>` not satisfied"); wrong Module type is a clear E0277. Messages carry the growing tuple type. `Send` errors: 22 lines, point at `on_action`. |
| Object-safety constraints hit | Needed `async-trait` (native async fn is not `dyn`-safe); `KIND` must be a method; no generic methods; handlers must be `'static + Send`; Capabilities as `Option<&dyn _>`. | None, but the reverse: nothing can be chosen at run time (channels from TOML, Modules toggled per config), and Deployments with different channel sets cannot share a collection. |
| Effort | Core: lower (maps and loops). Module authors: lower. | Core: higher (type lists, typestate, generic axum state). Module authors: must be comfortable with generics. |

## Recommendation

**Design A.** The seams customers touch (Module, Channel) read better, adding a Channel Capability or a Kernel
operation stays local, and runtime selection of channels/Modules from `deployment.toml` stays possible. B's
extra guarantee (missing Verification at compile time) is a single startup check in A that the example's
first test would hit anyway; its performance gain is a heap allocation per I/O call, invisible next to D1 and
HTTP round-trips. Keep from B: the native-RPITIT style could return later for internal, non-`dyn` helpers.

## Questions the owner must answer to pick

1. Must a Deployment switch Channels or Modules on/off from `deployment.toml` at startup without a rebuild?
   (Yes rules out B.)
2. Is "fails at startup / in the example's test" acceptable for a missing or duplicated piece, or must it be a
   compile error? (Compile error favours B, and even B only covers the missing-piece cases.)
3. Who writes customer Modules: you only, or developers at the customer or a contractor who should not need
   `impl<R: Runtime> Module<R>` and `::<R>` turbofish? (Others favour A.)
