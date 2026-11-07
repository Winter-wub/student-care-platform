//! Module: `register` is called once at build time and pushes everything
//! into a [`ModuleRegistrar`]; handlers are boxed closures.

use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use axum::routing::MethodRouter;

use crate::config::DeploymentConfig;
use crate::delivery::{ChannelAccountId, StudentId};
use crate::jobs::Jobs;
use crate::manifest::{Catalog, CatalogEntry, Job, JobSpec, MenuEntrySpec, Migration, Permission};
use crate::message::{ActionId, Locale, Message};
use crate::port_types::Statement;
use crate::{Result, Services};

mod kept;
mod mini_app;
mod notify;
mod setting_kind;
pub(crate) mod settings;
mod upload;
pub use kept::{CONFIRMED_KEEP_MS, KeptUpload, Uploads};
pub(crate) use kept::{RouteModule, sweep_uploads};
pub(crate) use mini_app::AppLinkSpec;
pub use notify::{
    DEFAULT_BUDGET, MAX_NOTIFY, Notice, Notified, NotifyDefer, NotifyFailure, NotifyOutcome,
    NotifyReport, NotifySkip, NotifyTo,
};
pub use settings::{MAX_LINE_CHARS, MAX_MULTILINE_CHARS, SettingKind, SettingValue, Settings};
pub(crate) use settings::{SettingSpec, absorb as absorb_settings, check as check_settings};
pub use upload::{MAX_UPLOAD_BYTES, SheetKind, SheetUpload, UPLOAD_KEEP_MS, UploadRejection};

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
    /// Enqueues Jobs (e.g. a send started from a back-office page).
    pub jobs: Jobs,
    /// The Deployment itself, for Kernel pages that call its services
    /// (the Link service needs the Channels and the catalog).
    pub(crate) deployment: Arc<crate::Deployment>,
}

impl PageState {
    /// A text in the default Locale.
    pub fn t(&self, key: &str) -> String {
        self.catalog.t(&self.config.default_locale, key)
    }

    /// The Module settings, read fresh on every call (#75).
    pub fn settings(&self) -> Settings {
        self.deployment.settings()
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
    /// The Deployment's configuration (e.g. `[announcements] latest`).
    pub config: Arc<DeploymentConfig>,
    /// The linked Student.
    pub student: StudentId,
    /// The account that pressed the button.
    pub account: ChannelAccountId,
    /// The Student's Locale.
    pub locale: Locale,
    /// The Action pressed: the registered id, or for a handler registered
    /// with [`ModuleRegistrar::action_prefix`] the whole id (prefix and
    /// suffix), which the handler parses.
    pub action: ActionId,
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
    /// The Deployment's configuration.
    pub config: Arc<DeploymentConfig>,
    /// Enqueues follow-up Jobs (e.g. one child Job per chunk).
    pub jobs: Jobs,
    /// This run's `job.id` and lock token (`None` outside a tick).
    pub(crate) run: Option<(String, String)>,
    /// The Deployment running the Job (a shallow copy), for Kernel Jobs
    /// that send through the Channels. Never keep a `JobContext` (or a
    /// clone) beyond the run: stored in a handler it would form a cycle
    /// (Deployment → registry → handler → context → Deployment) and leak.
    pub(crate) deployment: Arc<crate::Deployment>,
}

impl JobContext {
    /// Saves a short progress note on this Job (`job.progress`, shown by
    /// `/admin/jobs.json`; cut to 200 characters). Written only while this
    /// run still holds the lock; a no-op outside a tick.
    pub async fn progress(&self, note: &str) -> Result<()> {
        let Some((id, token)) = &self.run else {
            return Ok(());
        };
        let stmt = Statement::new(
            "UPDATE job SET progress = ? WHERE id = ? AND lock_token = ? AND state = 'running'",
        )
        .bind(crate::jobs::cut(note, 200))
        .bind(id.as_str())
        .bind(token.as_str());
        self.services.db.execute(stmt).await.map(|_| ())
    }

    /// The Module settings, read fresh on every call (#75).
    pub fn settings(&self) -> Settings {
        self.deployment.settings()
    }
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
    pub(crate) action_prefixes: Vec<(&'static str, Arc<dyn ActionHandler>)>,
    pub(crate) back_office: Vec<(&'static str, &'static str, MethodRouter<AppState>)>,
    /// The back-office upload routes.
    pub(crate) uploads: Vec<UploadSpec>,
    pub(crate) public: Vec<(&'static str, MethodRouter<AppState>)>,
    pub(crate) catalog: Vec<CatalogEntry>,
    pub(crate) jobs: Vec<(JobSpec, Arc<dyn JobHandler>)>,
    pub(crate) migrations: Vec<Migration>,
    pub(crate) mini_app_paths: Vec<&'static str>,
    pub(crate) nav: Vec<NavSpec>,
    /// Links on the Mini App home (#72).
    pub(crate) app_links: Vec<AppLinkSpec>,
    /// Typed settings (#75).
    pub(crate) settings: Vec<SettingSpec>,
}

/// A back-office upload route (paths under `/admin`, as registered).
#[derive(Debug, Clone, Copy)]
pub(crate) struct UploadSpec {
    pub(crate) path: &'static str,
    pub(crate) max_body: usize,
    /// The page a body over `max_body` is sent back to and the catalog key
    /// of that error flash; `None` answers the 413 page instead (#74).
    pub(crate) too_large: Option<(&'static str, &'static str)>,
}

/// A back-office page listed in the navigation and on `/admin`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NavSpec {
    pub(crate) permission: &'static str,
    pub(crate) path: &'static str,
    pub(crate) title_key: &'static str,
    pub(crate) order: u16,
}

impl ModuleRegistrar {
    pub(crate) fn new(module: &'static str) -> Self {
        Self {
            module,
            permissions: Vec::new(),
            menu: Vec::new(),
            actions: Vec::new(),
            action_prefixes: Vec::new(),
            back_office: Vec::new(),
            uploads: Vec::new(),
            public: Vec::new(),
            catalog: Vec::new(),
            jobs: Vec::new(),
            migrations: Vec::new(),
            mini_app_paths: Vec::new(),
            nav: Vec::new(),
            app_links: Vec::new(),
            settings: Vec::new(),
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

    /// A handler for every Action id starting with `prefix` (e.g.
    /// `announcements.ack.` for `announcements.ack.<announcement id>`), so
    /// a button can carry an argument in its id. The handler gets the
    /// whole id in [`ActionContext::action`] and must validate the suffix
    /// itself: an unknown or malformed suffix should answer with a polite
    /// Text, never an `Err` (which the Student would not see answered).
    ///
    /// `build()` refuses ([`crate::BuildError::DuplicateAction`]) a prefix
    /// that does not end with `.`, two prefixes where one starts with the
    /// other, and a prefix that covers an exact id registered with
    /// [`ModuleRegistrar::action`] or [`ModuleRegistrar::menu_entry`] by
    /// any Module. An exact id always wins; otherwise the longest
    /// matching prefix handles the Action.
    pub fn action_prefix(
        &mut self,
        prefix: &'static str,
        h: impl ActionHandler + 'static,
    ) -> &mut Self {
        self.action_prefixes.push((prefix, Arc::new(h)));
        self
    }

    /// A back-office page at `/admin{path}` (e.g. `path = "/reports"`),
    /// guarded: only Staff holding `permission` reach it. Chain methods on
    /// one `MethodRouter` (`get(..).post(..)`); `build()` refuses a path
    /// registered twice ([`crate::BuildError::DuplicateRoute`]) or one axum
    /// cannot mount ([`crate::BuildError::InvalidRoute`]).
    pub fn back_office(
        &mut self,
        permission: &'static str,
        path: &'static str,
        route: MethodRouter<AppState>,
    ) -> &mut Self {
        self.back_office.push((permission, path, route));
        self
    }

    /// Like [`ModuleRegistrar::back_office`] for a route that takes a
    /// `multipart/form-data` upload of up to `spec.max_body` bytes: the
    /// route gets that `DefaultBodyLimit` (for axum's `Multipart`), and the
    /// guard reads its body (for the `_csrf` part) up to that size, after
    /// the Permission check. A larger body is sent back to the page of
    /// `spec.too_large` with its error flash, or answered with the 413
    /// page when that is `None`. Every other back-office
    /// route gets the small `auth::MULTIPART_LIMIT` and a 413 page.
    pub(crate) fn back_office_upload(
        &mut self,
        permission: &'static str,
        spec: UploadSpec,
        route: MethodRouter<AppState>,
    ) -> &mut Self {
        let route = route.layer(axum::extract::DefaultBodyLimit::max(spec.max_body));
        self.uploads.push(spec);
        self.back_office(permission, spec.path, route)
    }

    /// Like [`ModuleRegistrar::back_office`], and also lists the page in
    /// the back-office navigation and on the `/admin` home for Staff
    /// holding `permission`: `title_key` is a catalog key (checked by
    /// `build()`), `nav_order` sorts the entries (lower first). `path` must
    /// not contain parameters ([`crate::BuildError::InvalidRoute`]).
    pub fn back_office_page(
        &mut self,
        permission: &'static str,
        path: &'static str,
        title_key: &'static str,
        nav_order: u16,
        route: MethodRouter<AppState>,
    ) -> &mut Self {
        self.nav.push(NavSpec {
            permission,
            path,
            title_key,
            order: nav_order,
        });
        self.back_office(permission, path, route)
    }

    /// A page outside `/admin` with no Staff session: Mini App pages (which
    /// check their own App Session) and public pages. `build()` refuses a
    /// path under `/admin` ([`crate::BuildError::InvalidRoute`]; use
    /// [`ModuleRegistrar::back_office`]), a path registered twice or one
    /// the Kernel owns ([`crate::BuildError::DuplicateRoute`]).
    pub fn public(&mut self, path: &'static str, route: MethodRouter<AppState>) -> &mut Self {
        self.public.push((path, route));
        self
    }

    /// Default texts.
    pub fn messages(&mut self, entries: &[CatalogEntry]) -> &mut Self {
        self.catalog.extend_from_slice(entries);
        self
    }

    /// A kind of Job and its handler. `spec.kind` must be unique across the
    /// Deployment ([`crate::BuildError::DuplicateJob`]); a `schedule` must
    /// be in the cron subset documented on [`crate::JobSpec::schedule`]
    /// ([`crate::BuildError::InvalidSchedule`]). Only registered kinds can
    /// be enqueued.
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
