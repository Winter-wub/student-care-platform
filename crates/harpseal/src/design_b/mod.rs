//! Design B: generics + static dispatch.
//!
//! A runtime is one type implementing [`Runtime`], whose associated types
//! name the concrete Db, FileStore, ... . Channels and Modules are kept in
//! type-level lists (nested tuples) built by the builder, so the compiler
//! sees every concrete type and nothing is boxed. Async trait methods use
//! native `impl Future + Send` (RPITIT), which is not `dyn`-compatible.

mod channel;
mod deployment;
mod module;

pub use channel::{Channel, ChannelSet, DeleteMessage, EditMessage, MiniApp, Unsupported};
pub use deployment::{Builder, Deployment, NoVerification};
pub use module::{ActionCx, BackOffice, BackOfficeState, JobCx, Manifest, Module, ModuleSet};

use std::future::Future;

use crate::Result;
use crate::delivery::{StudentId, Timestamp};
use crate::port_types::{Email, ExecResult, Rows, Statement};
use crate::verification::{Challenge, EmailOtp, StudentClaim};

/// SQLite-dialect database. No `begin`: atomic work is one `batch`.
pub trait Db: Send + Sync + 'static {
    fn query(&self, stmt: Statement) -> impl Future<Output = Result<Rows>> + Send;
    fn execute(&self, stmt: Statement) -> impl Future<Output = Result<ExecResult>> + Send;
    /// All statements commit or none do.
    fn batch(&self, stmts: Vec<Statement>) -> impl Future<Output = Result<Vec<ExecResult>>> + Send;
}

pub trait FileStore: Send + Sync + 'static {
    fn put(&self, key: &str, bytes: Vec<u8>, content_type: &str) -> impl Future<Output = Result<()>> + Send;
    fn get(&self, key: &str) -> impl Future<Output = Result<Option<Vec<u8>>>> + Send;
    fn delete(&self, key: &str) -> impl Future<Output = Result<()>> + Send;
}

pub trait EmailSender: Send + Sync + 'static {
    fn send(&self, email: Email) -> impl Future<Output = Result<()>> + Send;
}

pub trait HttpClient: Send + Sync + 'static {
    fn send(&self, req: http::Request<Vec<u8>>)
    -> impl Future<Output = Result<http::Response<Vec<u8>>>> + Send;
}

pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> Timestamp;
}

pub trait Random: Send + Sync + 'static {
    fn fill(&self, buf: &mut [u8]) -> Result<()>;
}

/// One type per runtime crate (`NativeRuntime`, `CloudflareRuntime`).
pub trait Runtime: Send + Sync + 'static {
    type Db: Db;
    type FileStore: FileStore;
    type Email: EmailSender;
    type Http: HttpClient;
    type Clock: Clock;
    type Random: Random;

    fn db(&self) -> &Self::Db;
    fn files(&self) -> &Self::FileStore;
    fn email(&self) -> &Self::Email;
    fn http(&self) -> &Self::Http;
    fn clock(&self) -> &Self::Clock;
    fn random(&self) -> &Self::Random;
}

/// A swappable way of proving which Student owns a Channel Account.
pub trait VerificationMethod<R: Runtime>: Send + Sync + 'static {
    const ID: &'static str;
    fn start(&self, rt: &R, claim: &StudentClaim) -> impl Future<Output = Result<Challenge>> + Send;
    /// `Ok(None)`: wrong answer.
    fn check(
        &self,
        rt: &R,
        challenge: &Challenge,
        answer: &str,
    ) -> impl Future<Output = Result<Option<StudentId>>> + Send;
}

impl<R: Runtime> VerificationMethod<R> for EmailOtp {
    const ID: &'static str = "email_otp";
    async fn start(&self, rt: &R, claim: &StudentClaim) -> Result<Challenge> {
        let _ = (rt.email(), claim);
        todo!("look up roster email, store hashed code, rt.email().send(..)")
    }
    async fn check(&self, _: &R, _: &Challenge, _: &str) -> Result<Option<StudentId>> {
        todo!("compare hashed code, enforce ttl_secs / max_attempts")
    }
}
