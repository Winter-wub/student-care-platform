//! Design A Module: `register` is called once at build time and pushes
//! everything into a `ModuleRegistrar`; handlers are boxed closures.

use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;

use crate::Services;
use crate::Result;
use crate::delivery::{ChannelAccountId, StudentId};
use crate::manifest::{Catalog, CatalogEntry, Job, JobSpec, MenuEntrySpec, Migration, Permission};
use crate::message::{ActionId, Locale, Message};

/// A switchable part of the Core or customer code.
pub trait Module: Send + Sync + 'static {
    /// Prefix for Permission ids, catalog keys, migrations and Jobs.
    fn name(&self) -> &'static str;
    /// Declare everything. Called once by `DeploymentBuilder::build`.
    fn register(&self, r: &mut ModuleRegistrar);
}

/// State given to back-office axum handlers.
pub struct BackOfficeState {
    pub services: Services,
    pub catalog: Arc<Catalog>,
}
pub type AppState = Arc<BackOfficeState>;

/// What an Action handler sees: the linked Student and the ports.
#[derive(Clone)]
pub struct ActionContext {
    pub services: Services,
    pub catalog: Arc<Catalog>,
    pub student: StudentId,
    pub account: ChannelAccountId,
    pub locale: Locale,
}

impl ActionContext {
    pub fn t(&self, key: &str) -> String {
        self.catalog.t(&self.locale, key)
    }
}

/// Handles one Action id; returns the messages to send back.
#[async_trait]
pub trait ActionHandler: Send + Sync {
    async fn handle(&self, cx: ActionContext) -> Result<Vec<Message>>;
}

#[async_trait]
impl<F, Fut> ActionHandler for F
where
    F: Fn(ActionContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<Vec<Message>>> + Send + 'static,
{
    async fn handle(&self, cx: ActionContext) -> Result<Vec<Message>> {
        (self)(cx).await
    }
}

#[derive(Clone)]
pub struct JobContext {
    pub services: Services,
    pub catalog: Arc<Catalog>,
}

/// Runs one Job. Must be idempotent: a Job may run again after a crash.
#[async_trait]
pub trait JobHandler: Send + Sync {
    async fn run(&self, cx: JobContext, job: Job) -> Result<()>;
}

#[async_trait]
impl<F, Fut> JobHandler for F
where
    F: Fn(JobContext, Job) -> Fut + Send + Sync,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    async fn run(&self, cx: JobContext, job: Job) -> Result<()> {
        (self)(cx, job).await
    }
}

/// Collects one Module's contributions. Validation (duplicate ids, unknown
/// Permissions) happens in `build()`, so mistakes are run-time errors.
pub struct ModuleRegistrar {
    pub(crate) module: &'static str,
    pub(crate) permissions: Vec<Permission>,
    pub(crate) menu: Vec<MenuEntrySpec>,
    pub(crate) actions: Vec<(ActionId, Arc<dyn ActionHandler>)>,
    pub(crate) back_office: Vec<(&'static str, axum::Router<AppState>)>,
    pub(crate) catalog: Vec<CatalogEntry>,
    pub(crate) jobs: Vec<(JobSpec, Arc<dyn JobHandler>)>,
    pub(crate) migrations: Vec<Migration>,
}

impl ModuleRegistrar {
    pub(crate) fn new(module: &'static str) -> Self {
        Self {
            module,
            permissions: Vec::new(),
            menu: Vec::new(),
            actions: Vec::new(),
            back_office: Vec::new(),
            catalog: Vec::new(),
            jobs: Vec::new(),
            migrations: Vec::new(),
        }
    }

    pub fn permission(&mut self, p: Permission) -> &mut Self {
        self.permissions.push(p);
        self
    }

    /// A Menu entry and the handler for its Action.
    pub fn menu_entry(&mut self, spec: MenuEntrySpec, h: impl ActionHandler + 'static) -> &mut Self {
        self.actions.push((spec.action.clone(), Arc::new(h)));
        self.menu.push(spec);
        self
    }

    /// An Action not on the Menu (e.g. a Card button).
    pub fn action(&mut self, id: ActionId, h: impl ActionHandler + 'static) -> &mut Self {
        self.actions.push((id, Arc::new(h)));
        self
    }

    /// Back-office pages, mounted under `/admin`, guarded by `permission`.
    pub fn back_office(&mut self, permission: &'static str, r: axum::Router<AppState>) -> &mut Self {
        self.back_office.push((permission, r));
        self
    }

    pub fn messages(&mut self, entries: &[CatalogEntry]) -> &mut Self {
        self.catalog.extend_from_slice(entries);
        self
    }

    pub fn job(&mut self, spec: JobSpec, h: impl JobHandler + 'static) -> &mut Self {
        self.jobs.push((spec, Arc::new(h)));
        self
    }

    /// Pass `include_str!("../sql/0001_x.sql")`.
    pub fn migration(&mut self, name: &'static str, sql: &'static str) -> &mut Self {
        let module = self.module;
        self.migrations.push(Migration { module, name, sql });
        self
    }
}
