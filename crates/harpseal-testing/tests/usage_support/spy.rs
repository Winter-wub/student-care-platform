//! A `Db` that counts `usage_daily` reads and can hold the watch's claim
//! batch (the one whose audit row is `usage.notify`) until a hook ran.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use harpseal::{Db, DeploymentConfig, ExecResult, Result, Rows, Statement};
use harpseal_testing::{Harness, TestApp};

pub type Hook = Pin<Box<dyn Future<Output = ()> + Send>>;

pub struct SpyDb {
    inner: Arc<dyn Db>,
    usage_reads: AtomicUsize,
    lone_package_reads: AtomicUsize,
    hook: Mutex<Option<Hook>>,
}

impl SpyDb {
    /// Queries that read `usage_daily` so far.
    pub fn usage_reads(&self) -> usize {
        self.usage_reads.load(Ordering::SeqCst)
    }

    /// Queries that read the LINE package row (#115) on their own, not in
    /// the same query as the `usage_daily` rows.
    pub fn lone_package_reads(&self) -> usize {
        self.lone_package_reads.load(Ordering::SeqCst)
    }

    /// A hook is still waiting for a claim.
    pub fn hook_pending(&self) -> bool {
        self.hook.lock().unwrap().is_some()
    }

    /// Runs `hook` once, just before the next claim batch commits.
    pub fn before_next_claim(&self, hook: Hook) {
        *self.hook.lock().unwrap() = Some(hook);
    }
}

#[async_trait]
impl Db for SpyDb {
    async fn query(&self, stmt: Statement) -> Result<Rows> {
        if stmt.sql.contains("FROM usage_daily") {
            self.usage_reads.fetch_add(1, Ordering::SeqCst);
        } else if stmt
            .params
            .iter()
            .any(|p| p.as_text() == Some("kernel.usage.line_package"))
        {
            self.lone_package_reads.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.query(stmt).await
    }
    async fn execute(&self, stmt: Statement) -> Result<ExecResult> {
        self.inner.execute(stmt).await
    }
    async fn batch(&self, stmts: Vec<Statement>) -> Result<Vec<ExecResult>> {
        let claim = stmts
            .first()
            .is_some_and(|s| s.params.iter().any(|p| p.as_text() == Some("usage.notify")));
        let hook = match claim {
            true => self.hook.lock().unwrap().take(),
            false => None,
        };
        if let Some(h) = hook {
            h.await;
        }
        self.inner.batch(stmts).await
    }
}

/// [`super::boot`], with the Deployment on a [`SpyDb`].
pub async fn boot_spied() -> (Harness, TestApp, Arc<SpyDb>) {
    let h = Harness::new();
    let spy = Arc::new(SpyDb {
        inner: h.db(),
        usage_reads: AtomicUsize::new(0),
        lone_package_reads: AtomicUsize::new(0),
        hook: Mutex::new(None),
    });
    let mut services = h.services();
    services.db = spy.clone();
    let config = DeploymentConfig::from_toml(super::CONFIG).unwrap();
    let d = h
        .builder()
        .config(config)
        .runtime(services)
        .channel(h.fake_channel().with_kind("fake2"))
        .build()
        .unwrap();
    let app = h.boot(d).await;
    (h, app, spy)
}
