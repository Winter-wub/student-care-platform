//! `harpseal`: the platform-neutral Core of a student care Deployment.
//!
//! A Deployment is assembled with [`Deployment::builder`] from a parsed
//! `deployment.toml` ([`DeploymentConfig`]), the runtime's [`Services`] (six
//! ports: [`Db`], [`FileStore`], [`EmailSender`], [`HttpClient`], [`Clock`],
//! [`Random`]), one or more [`Channel`]s, a [`VerificationMethod`] and any
//! number of [`Module`]s. `build()` returns a [`BuildError`] for every
//! misconfiguration; [`Deployment::migrate`] applies the Kernel's and the
//! Modules' migrations; [`Deployment::router`] is the axum `Router` both
//! runtimes serve.
//!
//! Every type a Deployment or Module author needs is re-exported here. The
//! modules [`render`] (helpers for Channel adapters), [`ui`] (the shared page
//! layout) and [`password`] (Staff password hashing) group free functions.
//! The Kernel's schema is not public API.
//!
//! Everything here compiles to `wasm32-unknown-unknown`; nothing names a
//! runtime type.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod channel;
mod config;
mod delivery;
mod deployment;
mod error;
mod file_key;
mod id;
mod kernel;
mod manifest;
mod message;
mod migrate;
mod module;
mod port_types;
mod ports;
mod resend;
mod sql_split;
mod verification;

pub mod password;
pub mod render;
pub mod ui;

#[cfg(feature = "fixtures")]
pub mod fixtures;

pub use channel::{Channel, DeleteMessage, EditMessage, MiniApp, Multicast, SendAttachment};
pub use config::{
    BrandingConfig, ConsentConfig, DeploymentConfig, InboundConfig, OtpConfig, QuotaConfig,
    RetentionConfig, StudentConfig,
};
pub use delivery::{
    BatchReceipt, Capabilities, Capability, ChannelAccountId, ChannelKind, Delivery, Inbound,
    IncomingFile, Metering, PlatformMessageId, ReplyToken, SendReceipt, StudentId, Timestamp,
    WebhookRequest,
};
pub use deployment::{Deployment, DeploymentBuilder, TickReport, WebhookReport};
pub use error::{BuildError, Error, Part, Result};
pub use file_key::check_file_key;
pub use manifest::{Catalog, CatalogEntry, Job, JobSpec, MenuEntrySpec, Migration, Permission};
pub use message::{
    ActionId, Attachment, Button, Card, CardList, Choices, Locale, Menu, MenuEntry, Message,
};
pub use migrate::{MigrationReport, migrate};
pub use module::{
    ActionContext, ActionHandler, AppState, JobContext, JobHandler, Module, ModuleRegistrar,
    PageState,
};
pub use port_types::{Email, ExecResult, Rows, Statement, Value};
pub use ports::{Clock, Db, EmailSender, FileStore, HttpClient, Random, Services};
pub use render::RenderCx;
pub use resend::{RESEND_ENDPOINT, ResendEmail};
pub use verification::{
    Challenge, EmailOtp, StudentClaim, Verdict, VerificationMethod, VerifyContext,
};
