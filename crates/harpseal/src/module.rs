//! Module: `register` is called once at build time and pushes everything
//! into a [`ModuleRegistrar`]; handlers are boxed closures.

use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;

use crate::config::DeploymentConfig;
use crate::delivery::{ChannelAccountId, StudentId};
use crate::manifest::{Catalog, CatalogEntry, Job, JobSpec, MenuEntrySpec, Migration, Permission};
use crate::message::{ActionId, Locale, Message};
use crate::{Result, Services};

/// A switchable part of the Core or customer code.
pub trait Module: Send + Sync + 'static {
    /// Prefix for Permission ids, catalog keys, migrations and Jobs.
    fn name(&self) -> &'static str;
    /// Declares everything. Called once by `DeploymentBuilder::build`.
    fn register(&self, r: &mut ModuleRegistrar);
}

/// State every page handler gets (back office, Mini App, public pages).
#[non_exhaustive]
pub struct PageState {
    /// The runtime ports.
    pub services: Services,
    /// Merged texts.
    pub catalog: Arc<Catalog>,
    /// The Deployment's configuration.
    pub config: Arc<DeploymentConfig>,
}

impl PageState {
    /// A text in the default Locale.
    pub fn t(&self, key: &str) -> String {
        self.catalog.t(&self.config.default_locale, key)
    }
}

/// The axum state type of every Module router.
pub type AppState = Arc<PageState>;

/// What an Action handler sees: the linked Student and the ports. Modules
/// never see Inbounds from accounts without a Link.
#[derive(Clone)]
#[non_exhaustive]
pub struct ActionContext {
    /// The runtime ports.
    pub services: Services,
    /// Merged texts.
    pub catalog: Arc<Catalog>,
    /// The linked Student.
    pub student: StudentId,
    /// The account that pressed the button.
    pub account: ChannelAccountId,
    /// The Student's Locale.
    pub locale: Locale,
}

impl ActionContext {
    /// A text in the Student's Locale.
    pub fn t(&self, key: &str) -> String {
        self.catalog.t(&self.locale, key)
    }
}

/// Handles one Action id; returns the messages to send back (one batch).
#[async_trait]
pub trait ActionHandler: Send + Sync {
    /// Runs the Action.
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

/// What a Job handler sees.
#[derive(Clone)]
#[non_exhaustive]
pub struct JobContext {
    /// The runtime ports.
    pub services: Services,
    /// Merged texts.
    pub catalog: Arc<Catalog>,
}

/// Runs one Job. Must be idempotent: a Job may run again after a crash.
#[async_trait]
pub trait JobHandler: Send + Sync {
    /// Runs the Job.
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
/// Permissions, Mini App availability) happens in `build()`.
pub struct ModuleRegistrar {
    pub(crate) module: &'static str,
    pub(crate) permissions: Vec<Permission>,
    pub(crate) menu: Vec<MenuEntrySpec>,
    pub(crate) actions: Vec<(ActionId, Arc<dyn ActionHandler>)>,
    pub(crate) back_office: Vec<(&'static str, axum::Router<AppState>)>,
    pub(crate) public: Vec<axum::Router<AppState>>,
    pub(crate) catalog: Vec<CatalogEntry>,
    pub(crate) jobs: Vec<(JobSpec, Arc<dyn JobHandler>)>,
    pub(crate) migrations: Vec<Migration>,
    pub(crate) mini_app_paths: Vec<&'static str>,
}

impl ModuleRegistrar {
    pub(crate) fn new(module: &'static str) -> Self {
        Self {
            module,
            permissions: Vec::new(),
            menu: Vec::new(),
            actions: Vec::new(),
            back_office: Vec::new(),
            public: Vec::new(),
            catalog: Vec::new(),
            jobs: Vec::new(),
            migrations: Vec::new(),
            mini_app_paths: Vec::new(),
        }
    }

    /// The Module being registered.
    pub fn module(&self) -> &'static str {
        self.module
    }

    /// Declares a Permission that Roles can grant.
    pub fn permission(&mut self, p: Permission) -> &mut Self {
        self.permissions.push(p);
        self
    }

    /// A Menu entry and the handler for its Action.
    pub fn menu_entry(
        &mut self,
        spec: MenuEntrySpec,
        h: impl ActionHandler + 'static,
    ) -> &mut Self {
        self.actions.push((spec.action.clone(), Arc::new(h)));
        self.menu.push(spec);
        self
    }

    /// An Action not on the Menu (e.g. a Card button).
    pub fn action(&mut self, id: ActionId, h: impl ActionHandler + 'static) -> &mut Self {
        self.actions.push((id, Arc::new(h)));
        self
    }

    /// Back-office pages, mounted under `/admin`. Every route is guarded:
    /// only Staff holding `permission` reach it.
    pub fn back_office(
        &mut self,
        permission: &'static str,
        r: axum::Router<AppState>,
    ) -> &mut Self {
        self.back_office.push((permission, r));
        self
    }

    /// Pages outside `/admin` with no Staff session: Mini App pages (which
    /// check their own App Session) and public pages.
    pub fn public(&mut self, r: axum::Router<AppState>) -> &mut Self {
        self.public.push(r);
        self
    }

    /// Declares that this Module sends `Button::MiniApp` to `path`.
    /// `build()` then requires every Channel to have a Mini App.
    pub fn mini_app_button(&mut self, path: &'static str) -> &mut Self {
        self.mini_app_paths.push(path);
        self
    }

    /// Default texts.
    pub fn messages(&mut self, entries: &[CatalogEntry]) -> &mut Self {
        self.catalog.extend_from_slice(entries);
        self
    }

    /// A kind of Job and its handler.
    pub fn job(&mut self, spec: JobSpec, h: impl JobHandler + 'static) -> &mut Self {
        self.jobs.push((spec, Arc::new(h)));
        self
    }

    /// A migration; pass `include_str!("../migrations/0001_x.sql")`.
    /// Applied in registration order, after the Kernel's.
    pub fn migration(&mut self, name: &'static str, sql: &'static str) -> &mut Self {
        let module = self.module;
        self.migrations.push(Migration { module, name, sql });
        self
    }
}
