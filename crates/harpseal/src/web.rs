//! The web crates a Module's pages are written with, at the versions this
//! Core is built with (#162): [`axum`] for routes, extractors and
//! responses, [`maud`] for markup types. A `MethodRouter` from another
//! axum is a different type to [`crate::ModuleRegistrar::back_office`], so
//! a Deployment takes axum from here and needs no `axum` dependency of its
//! own:
//!
//! ```
//! use harpseal::web::axum;
//! use axum::extract::State;
//! use axum::response::{Html, IntoResponse, Response};
//! use axum::routing::get;
//! use harpseal::{AppState, CurrentStaff, Module, ModuleRegistrar, ui};
//! use maud::html;
//!
//! async fn page(State(s): State<AppState>, staff: CurrentStaff) -> Response {
//!     let body = html! { p { "สวัสดี" } };
//!     let page = ui::back_office(&s, &staff, "Reports", body);
//!     Html(page.into_string()).into_response()
//! }
//!
//! struct Reports;
//! impl Module for Reports {
//!     fn name(&self) -> &'static str { "reports" }
//!     fn register(&self, r: &mut ModuleRegistrar) {
//!         r.back_office("reports.view", "/reports", get(page));
//!     }
//! }
//! ```
//!
//! `maud` is the exception: its `html!` macro expands to `extern crate
//! maud`, which only a direct dependency satisfies. A Deployment that
//! writes markup keeps `maud = "0.27"`, the Core's requirement, so Cargo
//! resolves one `maud` for both; [`maud::Markup`] here is then the same
//! type as the macro's. A different minor version (`0.28`) would be a
//! second crate whose `Markup` the [`crate::ui`] helpers do not take.
//!
//! Only the axum features the Core turns on are guaranteed, on native and
//! Workers alike: `form`, `query` and `multipart` (so `Form`, `Query` and
//! `Multipart`); `axum::http` is the `http` crate. Anything else (`Json`,
//! `MatchedPath`, …) needs a direct `axum = { version = "0.8",
//! default-features = false, features = [...] }` in the Deployment, which
//! Cargo merges into the same axum, so the types stay the Core's.
//!
//! axum 0.8 and maud 0.27 are part of this crate's public API: a Core
//! release that moves to another minor version of either is a breaking
//! release for Deployments.

pub use axum;
pub use maud;
