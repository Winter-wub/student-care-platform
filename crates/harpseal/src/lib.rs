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
//! layout), [`password`] (Staff password hashing) and [`audit`] (the audit
//! writer every Staff write goes through) group free functions;
//! [`backup`] dumps and restores a whole Deployment through its ports. Back-office
//! handlers extract [`CurrentStaff`] and [`RequestCtx`].
//! The Kernel's schema is not public API.
//!
//! Everything here compiles to `wasm32-unknown-unknown`; nothing names a
//! runtime type.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod auth;
mod channel;
mod client_ip;
mod config;
mod delivery;
mod deployment;
mod error;
mod file_key;
mod id;
mod inbound;
mod jobs;
mod kernel;
mod manifest;
mod message;
mod migrate;
mod module;
mod port_types;
mod ports;
mod report;
mod resend;
mod sql_split;
mod verification;

pub mod audit;
pub mod backup;
pub mod modules;
pub mod password;
pub mod render;
pub mod ui;

#[cfg(feature = "fixtures")]
pub mod fixtures;

pub use audit::{Actor, PeerIp, RequestCtx, Target};
pub use auth::{
    ADMINISTRATOR_ROLE, CurrentStaff, MIN_PASSWORD_CHARS, NewStaff, check_password_rules,
    create_staff, ensure_role, role_permissions, set_roles, set_temporary_password, staff_count,
};
pub use channel::{
    Channel, DeleteMessage, EditMessage, MiniApp, Multicast, ResumeToken, SendAttachment, WebClient,
};
pub use config::{
    AttrError, AttributeSpec, AttributeType, BrandingConfig, ConsentConfig, DeploymentConfig,
    InboundConfig, LocaleColumn, OtpConfig, QuotaConfig, RetentionConfig, RosterConfig,
    StaffConfig, StudentConfig, TagsColumn,
};
pub use delivery::{
    BatchReceipt, Capabilities, Capability, ChannelAccountId, ChannelKind, Delivery, Inbound,
    IncomingFile, Metering, PlatformMessageId, ReplyToken, SendReceipt, StudentId, Timestamp,
    WebhookRequest,
};
pub use deployment::{
    Deployment, DeploymentBuilder, ManySent, SendOptions, TickReport, WebhookReport,
};
pub use error::{BuildError, Error, Part, Result};
pub use file_key::check_file_key;
pub use jobs::{JobId, Jobs};
pub use kernel::admin::texts_api as texts;
pub use kernel::audience;
pub use kernel::links;
pub use kernel::links::LinkChange;
pub use kernel::roster::api as roster;
pub use kernel::students;
pub use manifest::{Catalog, CatalogEntry, Job, JobSpec, MenuEntrySpec, Migration, Permission};
pub use message::{
    ActionId, Attachment, Button, Card, CardList, Choices, Locale, Menu, MenuEntry, MenuKey,
    Message,
};
pub use migrate::{MigrationReport, migrate};
pub use module::{
    ActionContext, ActionHandler, AppState, JobContext, JobHandler, Module, ModuleRegistrar,
    PageState,
};
pub use port_types::{Email, ExecResult, Rows, Statement, Value};
pub use ports::{Clock, Db, EmailSender, FileStore, HttpClient, Random, Services};
pub use render::RenderCx;
pub use report::ErrorReport;
pub use resend::{RESEND_ENDPOINT, ResendEmail};
pub use verification::{
    Challenge, EmailOtp, StudentClaim, Verdict, VerificationMethod, VerifyContext,
};
