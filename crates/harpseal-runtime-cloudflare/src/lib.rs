//! STUB (prototype). Cloudflare Workers runtime via workers-rs.
//!
//! Will hold: D1 -> `Db` (query / execute / batch; no `begin`), R2 ->
//! `FileStore`, email binding or HTTP email API -> `EmailSender`,
//! `worker::Fetch` -> `HttpClient`, `Date.now` -> `Clock`,
//! `crypto.getRandomValues` -> `Random`, for whichever design is chosen.
//! JS-backed futures are `!Send`; the Core's traits require `Send`, so impls
//! wrap them (workers-rs `SendFuture` / `#[worker::send]`; safe because a
//! Worker isolate is single-threaded).
//!
//! `worker = "0.8"` (0.8.7) with features http, axum, d1 compiles here for
//! both wasm32-unknown-unknown and the host target on rustc 1.99.1.

pub use worker;
