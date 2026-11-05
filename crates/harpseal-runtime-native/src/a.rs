//! Design A wiring: concrete types behind `Arc<dyn _>` in `Services`.

use std::sync::Arc;

use async_trait::async_trait;
use harpseal::delivery::Timestamp;
use harpseal::design_a::{self as a, Services};
use harpseal::port_types::{Email, ExecResult, Rows, Statement};
use harpseal::Result;

use crate::*;

#[async_trait]
impl a::Db for SqliteDb {
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

#[async_trait]
impl a::FileStore for LocalFiles {
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

#[async_trait]
impl a::EmailSender for Smtp {
    async fn send(&self, _email: Email) -> Result<()> {
        todo!("lettre SMTP transport to self.url")
    }
}

#[async_trait]
impl a::HttpClient for NativeHttp {
    async fn send(&self, _req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
        todo!("reqwest / hyper client")
    }
}

impl a::Clock for SystemClock {
    fn now(&self) -> Timestamp {
        SystemClock::now_sync()
    }
}

impl a::Random for OsRandom {
    fn fill(&self, buf: &mut [u8]) -> Result<()> {
        fill_random(buf)
    }
}

/// Plug for Design A: `Deployment::builder().runtime(services(&s)?)`.
/// Returns the Db too so the binary can run migrations.
pub fn services(s: &NativeSettings) -> Result<(Services, Arc<SqliteDb>)> {
    let db = Arc::new(SqliteDb::open(&s.sqlite_path)?);
    let services = Services {
        db: db.clone(),
        files: Arc::new(LocalFiles { root: s.files_root.clone() }),
        email: Arc::new(Smtp { url: s.smtp_url.clone() }),
        http: Arc::new(NativeHttp),
        clock: Arc::new(SystemClock),
        random: Arc::new(OsRandom),
    };
    Ok((services, db))
}
