//! harpseal — the Core. THROWAWAY PROTOTYPE (issues #4 and #3).
//!
//! Design A from issue #4: trait objects + runtime registry. Every seam is an
//! object-safe trait stored as `Arc<dyn _>`. A runtime crate hands the Core a
//! [`Services`] value; Modules call [`ModuleRegistrar`] methods from
//! [`Module::register`]; the Core dispatches by id at run time.

pub mod config;
pub mod delivery;
pub mod error;
pub mod manifest;
pub mod message;
pub mod password;
pub mod port_types;
pub mod render;
pub mod ui;
pub mod verification;

#[cfg(feature = "fixtures")]
pub mod fixtures;

mod channel;
mod deployment;
mod module;
mod ports;

pub use channel::{Channel, DeleteMessage, EditMessage, MiniApp};
pub use deployment::{Deployment, DeploymentBuilder};
pub use error::{BuildError, Error, Result};
pub use module::{
    ActionContext, ActionHandler, AppState, BackOfficeState, JobContext, JobHandler, Module,
    ModuleRegistrar,
};
pub use ports::{
    Clock, Db, EmailSender, FileStore, HttpClient, Random, Services, VerificationMethod,
};

pub(crate) mod kernel;
