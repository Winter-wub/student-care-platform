//! Design A: trait objects + runtime registry.
//!
//! Every seam is an object-safe trait stored as `Arc<dyn _>`. A runtime crate
//! hands the Core a [`Services`] value; Modules call [`ModuleRegistrar`]
//! methods from [`Module::register`]; the Core keeps the result in maps and
//! dispatches by id at run time.

mod channel;
mod deployment;
mod module;

pub use channel::{Channel, DeleteMessage, EditMessage, MiniApp};
pub use deployment::{Deployment, DeploymentBuilder};
pub use module::{
    ActionContext, ActionHandler, AppState, BackOfficeState, JobContext, JobHandler, Module,
    ModuleRegistrar,
};

use std::sync::Arc;

use async_trait::async_trait;

use crate::Result;
use crate::delivery::{StudentId, Timestamp};
use crate::port_types::{Email, ExecResult, Rows, Statement};
use crate::verification::{Challenge, EmailOtp, StudentClaim};

/// SQLite-dialect database. D1 has no interactive transactions, so there is
/// no `begin`: atomic work is one [`Db::batch`].
#[async_trait]
pub trait Db: Send + Sync {
    async fn query(&self, stmt: Statement) -> Result<Rows>;
    async fn execute(&self, stmt: Statement) -> Result<ExecResult>;
    /// All statements commit or none do.
    async fn batch(&self, stmts: Vec<Statement>) -> Result<Vec<ExecResult>>;
}

/// Object storage (R2 / S3-compatible / local directory).
#[async_trait]
pub trait FileStore: Send + Sync {
    async fn put(&self, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()>;
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    async fn delete(&self, key: &str) -> Result<()>;
}

#[async_trait]
pub trait EmailSender: Send + Sync {
    async fn send(&self, email: Email) -> Result<()>;
}

/// Outgoing HTTP for Channel adapters (worker::Fetch / reqwest / ...).
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn send(&self, req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// Cryptographically secure bytes (crypto.getRandomValues / OS RNG).
pub trait Random: Send + Sync {
    fn fill(&self, buf: &mut [u8]) -> Result<()>;
}

/// Everything a runtime crate provides. Cheap to clone.
#[derive(Clone)]
pub struct Services {
    pub db: Arc<dyn Db>,
    pub files: Arc<dyn FileStore>,
    pub email: Arc<dyn EmailSender>,
    pub http: Arc<dyn HttpClient>,
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn Random>,
}

/// A swappable way of proving which Student owns a Channel Account.
#[async_trait]
pub trait VerificationMethod: Send + Sync {
    fn id(&self) -> &'static str;
    async fn start(&self, services: &Services, claim: &StudentClaim) -> Result<Challenge>;
    /// `Ok(None)`: wrong answer (the Kernel counts attempts).
    async fn check(
        &self,
        services: &Services,
        challenge: &Challenge,
        answer: &str,
    ) -> Result<Option<StudentId>>;
}

#[async_trait]
impl VerificationMethod for EmailOtp {
    fn id(&self) -> &'static str {
        "email_otp"
    }
    async fn start(&self, services: &Services, claim: &StudentClaim) -> Result<Challenge> {
        let _ = (services, claim, self.code_digits);
        todo!("look up roster email, store hashed code, services.email.send(..)")
    }
    async fn check(&self, _: &Services, _: &Challenge, _: &str) -> Result<Option<StudentId>> {
        todo!("compare hashed code, enforce ttl_secs / max_attempts")
    }
}
