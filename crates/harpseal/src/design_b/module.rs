//! Design B Module: contributions are associated consts plus dispatch
//! methods; a Module is generic over the Runtime it runs on.

use std::future::Future;
use std::sync::Arc;

use super::Runtime;
use crate::Result;
use crate::delivery::{ChannelAccountId, StudentId};
use crate::manifest::{Catalog, CatalogEntry, Job, JobSpec, MenuEntrySpec, Migration, Permission};
use crate::message::{ActionId, Locale, Message};

pub struct BackOfficeState<R> {
    pub runtime: Arc<R>,
    pub catalog: Arc<Catalog>,
}
pub type BackOffice<R> = axum::Router<Arc<BackOfficeState<R>>>;

pub struct ActionCx<R> {
    pub runtime: Arc<R>,
    pub catalog: Arc<Catalog>,
    pub student: StudentId,
    pub account: ChannelAccountId,
    pub locale: Locale,
}

impl<R> ActionCx<R> {
    pub fn t(&self, key: &str) -> String {
        self.catalog.t(&self.locale, key)
    }
}

pub struct JobCx<R> {
    pub runtime: Arc<R>,
    pub catalog: Arc<Catalog>,
}

/// A switchable part of the Core or customer code. Declarative parts are
/// consts (checked for type, not for cross-Module uniqueness); behaviour is
/// a `match` on the Action id / Job kind inside the Module.
pub trait Module<R: Runtime>: Send + Sync + 'static {
    const NAME: &'static str;
    const PERMISSIONS: &'static [Permission] = &[];
    const CATALOG: &'static [CatalogEntry] = &[];
    const JOBS: &'static [JobSpec] = &[];
    const MIGRATIONS: &'static [Migration] = &[];

    /// Not a const: `ActionId::new` validates length at run time.
    fn menu(&self) -> Vec<MenuEntrySpec> {
        Vec::new()
    }
    /// Pages under `/admin`, guarded by the returned Permission id.
    fn back_office(&self) -> Option<(&'static str, BackOffice<R>)> {
        None
    }
    /// `Ok(None)`: not my Action, ask the next Module.
    fn on_action(&self, cx: &ActionCx<R>, action: &ActionId)
    -> impl Future<Output = Result<Option<Vec<Message>>>> + Send {
        let _ = (cx, action);
        async { Ok(None) }
    }
    /// `Ok(false)`: not my Job kind.
    fn run_job(&self, cx: &JobCx<R>, job: &Job) -> impl Future<Output = Result<bool>> + Send {
        let _ = (cx, job);
        async { Ok(false) }
    }
}

/// Everything collected from a ModuleSet at build time.
pub struct Manifest<R> {
    pub permissions: Vec<Permission>,
    pub menu: Vec<MenuEntrySpec>,
    pub catalog: Vec<CatalogEntry>,
    pub jobs: Vec<JobSpec>,
    pub migrations: Vec<Migration>,
    pub back_office: Vec<(&'static str, &'static str, BackOffice<R>)>,
}

impl<R> Default for Manifest<R> {
    fn default() -> Self {
        Self {
            permissions: vec![],
            menu: vec![],
            catalog: vec![],
            jobs: vec![],
            migrations: vec![],
            back_office: vec![],
        }
    }
}

/// Type-level list of Modules: `()` or `(Head, Tail)`.
pub trait ModuleSet<R: Runtime>: Send + Sync + 'static {
    fn collect(&self, out: &mut Manifest<R>);
    fn on_action(&self, cx: &ActionCx<R>, action: &ActionId)
    -> impl Future<Output = Result<Option<Vec<Message>>>> + Send;
    fn run_job(&self, cx: &JobCx<R>, job: &Job) -> impl Future<Output = Result<bool>> + Send;
}

impl<R: Runtime> ModuleSet<R> for () {
    fn collect(&self, _: &mut Manifest<R>) {}
    async fn on_action(&self, _: &ActionCx<R>, _: &ActionId) -> Result<Option<Vec<Message>>> {
        Ok(None)
    }
    async fn run_job(&self, _: &JobCx<R>, _: &Job) -> Result<bool> {
        Ok(false)
    }
}

impl<R: Runtime, H: Module<R>, T: ModuleSet<R>> ModuleSet<R> for (H, T) {
    fn collect(&self, out: &mut Manifest<R>) {
        self.1.collect(out); // tail first: registration order
        out.permissions.extend_from_slice(H::PERMISSIONS);
        out.catalog.extend_from_slice(H::CATALOG);
        out.jobs.extend_from_slice(H::JOBS);
        out.migrations.extend_from_slice(H::MIGRATIONS);
        out.menu.extend(self.0.menu());
        if let Some((perm, r)) = self.0.back_office() {
            out.back_office.push((H::NAME, perm, r));
        }
    }
    async fn on_action(&self, cx: &ActionCx<R>, action: &ActionId) -> Result<Option<Vec<Message>>> {
        match self.0.on_action(cx, action).await? {
            Some(m) => Ok(Some(m)),
            None => self.1.on_action(cx, action).await,
        }
    }
    async fn run_job(&self, cx: &JobCx<R>, job: &Job) -> Result<bool> {
        Ok(self.0.run_job(cx, job).await? || self.1.run_job(cx, job).await?)
    }
}
