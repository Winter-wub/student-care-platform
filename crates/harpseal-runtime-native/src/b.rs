//! Design B wiring: one `NativeRuntime` type names every concrete port.

use harpseal::delivery::Timestamp;
use harpseal::design_b as b;
use harpseal::port_types::{Email, ExecResult, Rows, Statement};
use harpseal::Result;

use crate::*;

impl b::Db for SqliteDb {
    async fn query(&self, s: Statement) -> Result<Rows> {
        self.query_sync(&s)
    }
    async fn execute(&self, s: Statement) -> Result<ExecResult> {
        self.execute_sync(&s)
    }
    async fn batch(&self, s: Vec<Statement>) -> Result<Vec<ExecResult>> {
        self.batch_sync(&s)
    }
}

impl b::FileStore for LocalFiles {
    async fn put(&self, key: &str, bytes: Vec<u8>, _ct: &str) -> Result<()> {
        self.put_sync(key, &bytes)
    }
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.get_sync(key)
    }
    async fn delete(&self, key: &str) -> Result<()> {
        self.delete_sync(key)
    }
}

impl b::EmailSender for Smtp {
    async fn send(&self, _email: Email) -> Result<()> {
        todo!("lettre SMTP transport")
    }
}

impl b::HttpClient for NativeHttp {
    async fn send(&self, _req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
        todo!("reqwest / hyper client")
    }
}

impl b::Clock for SystemClock {
    fn now(&self) -> Timestamp {
        SystemClock::now_sync()
    }
}

impl b::Random for OsRandom {
    fn fill(&self, buf: &mut [u8]) -> Result<()> {
        fill_random(buf)
    }
}

/// Plug for Design B: `Deployment::builder(NativeRuntime::open(&s)?)`.
pub struct NativeRuntime {
    pub db: SqliteDb,
    pub files: LocalFiles,
    pub email: Smtp,
    pub http: NativeHttp,
}

impl NativeRuntime {
    pub fn open(s: &NativeSettings) -> Result<Self> {
        Ok(Self {
            db: SqliteDb::open(&s.sqlite_path)?,
            files: LocalFiles { root: s.files_root.clone() },
            email: Smtp { url: s.smtp_url.clone() },
            http: NativeHttp,
        })
    }
}

impl b::Runtime for NativeRuntime {
    type Db = SqliteDb;
    type FileStore = LocalFiles;
    type Email = Smtp;
    type Http = NativeHttp;
    type Clock = SystemClock;
    type Random = OsRandom;

    fn db(&self) -> &SqliteDb {
        &self.db
    }
    fn files(&self) -> &LocalFiles {
        &self.files
    }
    fn email(&self) -> &Smtp {
        &self.email
    }
    fn http(&self) -> &NativeHttp {
        &self.http
    }
    fn clock(&self) -> &SystemClock {
        &SystemClock
    }
    fn random(&self) -> &OsRandom {
        &OsRandom
    }
}
