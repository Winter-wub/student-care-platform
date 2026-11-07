//! The six runtime ports and [`Services`]. Every port is an object-safe
//! trait held as `Arc<dyn _>`; a runtime crate builds the `Services` and the
//! Core never names a runtime type (Design A, issue #4).

use std::sync::Arc;

use async_trait::async_trait;

use crate::Result;
use crate::delivery::Timestamp;
use crate::port_types::{Email, ExecResult, Rows, Statement};

/// SQLite-dialect database. D1 has no interactive transactions, so there is
/// no `begin`: atomic work is one [`Db::batch`].
#[async_trait]
pub trait Db: Send + Sync {
    /// Runs one statement and returns its rows.
    async fn query(&self, stmt: Statement) -> Result<Rows>;
    /// Runs one statement that returns no rows.
    async fn execute(&self, stmt: Statement) -> Result<ExecResult>;
    /// Runs every statement in order; all commit or none do. The only
    /// transaction primitive.
    async fn batch(&self, stmts: Vec<Statement>) -> Result<Vec<ExecResult>>;
}

/// Object storage (R2, S3-compatible storage, a local directory).
#[async_trait]
pub trait FileStore: Send + Sync {
    /// Stores `bytes` under `key`, replacing any previous object.
    async fn put(&self, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()>;
    /// Reads the object, `None` when absent.
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    /// Deletes the object; deleting a missing key is not an error.
    async fn delete(&self, key: &str) -> Result<()>;
}

/// Outgoing email (Resend HTTP API, SMTP).
#[async_trait]
pub trait EmailSender: Send + Sync {
    /// Sends one email.
    async fn send(&self, email: Email) -> Result<()>;

    /// Sends one email with an idempotency `key` (at most 256 bytes): a
    /// sender whose provider de-duplicates on it ([`EmailSender::dedupes`])
    /// sends a repeat with the same key only once. The default ignores the
    /// key and calls [`EmailSender::send`].
    async fn send_keyed(&self, email: Email, key: &str) -> Result<()> {
        let _ = key;
        self.send(email).await
    }

    /// Whether [`EmailSender::send_keyed`] de-duplicates on its key (Resend:
    /// the `Idempotency-Key` header, 24 hours). Default `false` (SMTP).
    fn dedupes(&self) -> bool {
        false
    }
}

/// Outgoing HTTP for Channel adapters and email APIs.
#[async_trait]
pub trait HttpClient: Send + Sync {
    /// Sends one request and returns the whole response.
    async fn send(&self, req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>>;
}

/// Wall-clock time. Frozen within a request on Workers: never measure
/// durations with it.
pub trait Clock: Send + Sync {
    /// Now, in milliseconds UTC.
    fn now(&self) -> Timestamp;
}

/// Cryptographically secure bytes (`crypto.getRandomValues`, the OS RNG).
pub trait Random: Send + Sync {
    /// Fills `buf` with random bytes.
    fn fill(&self, buf: &mut [u8]) -> Result<()>;
}

/// Everything a runtime crate provides. Cheap to clone.
#[derive(Clone)]
pub struct Services {
    /// The database.
    pub db: Arc<dyn Db>,
    /// Object storage.
    pub files: Arc<dyn FileStore>,
    /// Outgoing email.
    pub email: Arc<dyn EmailSender>,
    /// Outgoing HTTP.
    pub http: Arc<dyn HttpClient>,
    /// Wall-clock time.
    pub clock: Arc<dyn Clock>,
    /// Secure random bytes.
    pub random: Arc<dyn Random>,
}

impl Services {
    /// Now, in milliseconds UTC (ADR-0007 timestamps).
    pub fn now_ms(&self) -> i64 {
        self.clock.now().0
    }

    /// A new primary key: a UUIDv7 as 36-char lowercase text, from the
    /// Clock and Random ports (ADR-0007).
    pub fn new_id(&self) -> Result<String> {
        let mut rand = [0u8; 10];
        self.random.fill(&mut rand)?;
        Ok(crate::id::uuid_v7(self.now_ms(), rand))
    }

    /// `n` random bytes as lowercase hex (tokens, salts).
    pub fn random_hex(&self, n: usize) -> Result<String> {
        let mut buf = vec![0u8; n];
        self.random.fill(&mut buf)?;
        Ok(crate::id::hex(&buf))
    }
}

impl core::fmt::Debug for Services {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Services { .. }")
    }
}
