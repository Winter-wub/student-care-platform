//! A built Deployment: dispatches through the maps the builder validated.

mod background;
mod builder;
pub(crate) mod day;
mod inbound;
mod routes;
mod send;
pub(crate) use day::civil_day;
mod send_many;
mod tick;
mod web;

use std::collections::HashMap;
use std::sync::Arc;

pub use background::{Background, BackgroundTask, Slot, SlotFuture, Spawner};
pub use builder::DeploymentBuilder;
pub use send::SendOptions;
pub use send_many::ManySent;
pub use tick::TickReport;

use crate::config::DeploymentConfig;
use crate::delivery::{ChannelAccountId, PlatformMessageId};
use crate::manifest::{Catalog, Job, JobSpec, MenuEntrySpec, Migration, Permission};
use crate::message::{ActionId, Locale, Menu, MenuEntry, Message};
use crate::migrate::MigrationReport;
use crate::{
    ActionHandler, Channel, Error, JobContext, JobHandler, Result, Services, VerificationMethod,
    kernel,
};

/// One customer's running system: the Core, its configuration, Channels and
/// Modules. Build it with [`Deployment::builder`]; serve it with
/// [`Deployment::router`].
#[derive(Clone)]
pub struct Deployment {
    config: Arc<DeploymentConfig>,
    services: Services,
    queue: crate::jobs::Jobs,
    channels: Vec<Arc<dyn Channel>>,
    verification: Arc<dyn VerificationMethod>,
    catalog: Arc<Catalog>,
    registry: Arc<Registry>,
    /// Claimed webhook events waiting per chat account (#97).
    inbound_queues: background::AccountQueues,
}

/// What the Modules registered, after validation.
struct Registry {
    permissions: Vec<Permission>,
    menu: Vec<MenuEntrySpec>,
    actions: HashMap<ActionId, (&'static str, Arc<dyn ActionHandler>)>,
    /// (prefix, module, handler) from `action_prefix`, checked not to
    /// overlap each other or any exact id.
    action_prefixes: Vec<(&'static str, &'static str, Arc<dyn ActionHandler>)>,
    jobs: HashMap<&'static str, (JobSpec, Arc<dyn JobHandler>)>,
    /// Parsed `JobSpec::schedule`s, in registration order (#23).
    schedules: Vec<(&'static str, crate::jobs::cron::Schedule)>,
    migrations: Vec<Migration>,
    /// (module, permission, path under `/admin`, handler).
    back_office: Vec<(&'static str, &'static str, &'static str, Route)>,
    /// The upload routes (#46).
    uploads: Vec<crate::module::UploadSpec>,
    /// (module, path, handler).
    public: Vec<(&'static str, &'static str, Route)>,
    /// Back-office navigation, sorted (#21).
    nav: Vec<crate::auth::NavItem>,
    /// Links on the Mini App home, sorted (#72).
    app_links: Vec<crate::kernel::app::AppLink>,
    /// Module settings, in registration order (#75).
    settings: Arc<[crate::module::SettingSpec]>,
}

/// One registered route's handler.
pub(crate) type Route = axum::routing::MethodRouter<crate::AppState>;

/// What one webhook call did, after its signature and body were accepted.
#[derive(Debug)]
#[non_exhaustive]
pub struct WebhookReport {
    /// Events in the call.
    pub events: usize,
    /// The events that failed (kind and error); each was logged as a
    /// Security Event `inbound.failed` and did not stop the others.
    pub failed: Vec<(&'static str, Error)>,
    /// Events skipped because their event id was already claimed (a
    /// platform retry or redelivery). A redelivery of a claim that never
    /// finished counts here and in `failed` (#44).
    pub duplicates: usize,
}

impl Deployment {
    /// Starts assembling a Deployment.
    pub fn builder() -> DeploymentBuilder {
        DeploymentBuilder::default()
    }

    /// The parsed `deployment.toml`.
    pub fn config(&self) -> &DeploymentConfig {
        &self.config
    }

    /// The configuration, shared (for the contexts handed to Modules).
    pub(crate) fn config_arc(&self) -> Arc<DeploymentConfig> {
        self.config.clone()
    }
    /// The runtime ports.
    pub fn services(&self) -> &Services {
        &self.services
    }
    /// Merged texts: Module defaults, then `[texts]`.
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }
    /// Every Permission declared, Kernel first.
    pub fn permissions(&self) -> &[Permission] {
        &self.registry.permissions
    }
    /// The Verification Method.
    pub fn verification(&self) -> &dyn VerificationMethod {
        self.verification.as_ref()
    }
    /// The Module settings (#75): each read returns the latest saved value.
    pub fn settings(&self) -> crate::Settings {
        crate::Settings::new(self.services.clone(), self.registry.settings.clone())
    }
    /// Saves a Module setting as the system (#103): for the code that holds
    /// the `Deployment`, a seed or the CLI, never for Module code (pages
    /// and Jobs only get [`crate::Settings`], whose `save` needs Staff with
    /// the setting's Permission). The same kind check and `setting.update`
    /// audit row as `/admin/settings`, with actor `system` and the
    /// `ctx` (use `RequestCtx::cli().seeded()` for demo data). The same
    /// value again writes nothing; a row changed meanwhile is
    /// [`crate::Error::Conflict`]; an undeclared key or a refused value is
    /// [`crate::Error::Invalid`].
    pub async fn save_setting_as_system(
        &self,
        key: &str,
        value: &str,
        ctx: &crate::RequestCtx,
    ) -> crate::Result<()> {
        self.settings().save_as_system(key, value, ctx).await
    }
    /// Kernel migrations first, then Modules in registration order.
    pub fn migrations(&self) -> &[Migration] {
        &self.registry.migrations
    }

    /// Applies every pending migration to this Deployment's `Db`, then
    /// merges `[roles]` into the Roles tables (add-only). Running it again
    /// is a no-op.
    pub async fn migrate(&self) -> Result<MigrationReport> {
        let now = self.services.now_ms();
        let mut report =
            crate::migrate::migrate(self.services.db.as_ref(), self.migrations(), now).await?;
        report.notices = crate::auth::roles::seed(&self.services, &self.config.roles).await?;
        Ok(report)
    }

    /// The Channel of `kind`.
    pub fn channel(&self, kind: &str) -> Option<&dyn Channel> {
        self.channels
            .iter()
            .find(|c| c.kind().as_str() == kind)
            .map(|c| c.as_ref())
    }

    /// Every Channel, in registration order.
    pub(crate) fn channels(&self) -> &[Arc<dyn Channel>] {
        &self.channels
    }

    pub(crate) fn channel_or_err(&self, kind: &str) -> Result<&dyn Channel> {
        self.channel(kind)
            .ok_or_else(|| Error::Invalid(format!("no channel {kind}")))
    }

    /// The unlocked Menu in `locale`: the Modules' entries by `order`,
    /// then the Kernel's "unlink" entry (`kernel.link.unlink`) when
    /// `[student] self_unlink` is on. Its key is that of the effective
    /// Locale ([`Catalog::effective`], [`MenuKey::for_locale`]): a Student
    /// in `TH` or `en-US` with no texts of their own reads the default
    /// Locale, and gets its Menu and key. `build()` checked that every
    /// catalog Locale has a key of its own.
    ///
    /// [`MenuKey::for_locale`]: crate::MenuKey::for_locale
    pub fn menu_for(&self, locale: &Locale) -> Menu {
        self.menu_with(locale, |key| self.catalog.t(locale, key))
    }

    /// The unlocked Menu in `locale` with labels from `t`, a label key to
    /// its text (the chat flow passes texts with `setting` overrides);
    /// keyed as in [`Deployment::menu_for`], whatever the labels.
    pub fn menu_with(&self, locale: &Locale, t: impl Fn(&str) -> String) -> Menu {
        let mut entries: Vec<MenuEntry> = self
            .registry
            .menu
            .iter()
            .map(|e| MenuEntry {
                label: t(e.label_key),
                action: e.action.clone(),
            })
            .collect();
        if self.config.student.self_unlink
            && let Ok(action) = ActionId::new(kernel::flow::actions::UNLINK)
        {
            entries.push(MenuEntry {
                label: t("kernel.menu.unlink"),
                action,
            });
        }
        let menu = Menu::new(entries);
        let key = crate::MenuKey::for_locale(&self.catalog.effective(locale));
        // Unreachable: the effective Locale is a catalog Locale, and
        // `build()` refused any without a key (`BuildError::MenuKey`).
        debug_assert!(key.is_ok(), "{locale:?}: {key:?}");
        match key {
            Ok(key) => menu.with_key(key),
            Err(_) => menu,
        }
    }

    /// The catalog keys of the unlocked Menu's labels: the Modules'
    /// entries, then `kernel.menu.unlink` when `[student] self_unlink` is
    /// on. A back-office save of one of them re-links the Menu (#66).
    pub(crate) fn menu_label_keys(&self) -> Vec<&'static str> {
        let mut keys: Vec<&'static str> = self.registry.menu.iter().map(|e| e.label_key).collect();
        if self.config.student.self_unlink {
            keys.push("kernel.menu.unlink");
        }
        keys
    }

    /// The Module and handler registered for `action`: the exact id
    /// first, then the longest `action_prefix` it starts with.
    pub(crate) fn module_action(
        &self,
        action: &ActionId,
    ) -> Option<(&'static str, Arc<dyn ActionHandler>)> {
        if let Some((m, h)) = self.registry.actions.get(action) {
            return Some((*m, h.clone()));
        }
        resolve_prefix(&self.registry.action_prefixes, action.as_str())
    }

    /// The Mini App home's links, sorted (#72).
    pub(crate) fn app_links(&self) -> &[crate::kernel::app::AppLink] {
        &self.registry.app_links
    }

    /// The merged catalog, shared.
    pub(crate) fn catalog_arc(&self) -> Arc<Catalog> {
        self.catalog.clone()
    }

    /// Uses the edit Capability when present, otherwise sends a new message.
    pub async fn update_message(
        &self,
        to: &ChannelAccountId,
        id: &PlatformMessageId,
        msg: &Message,
        locale: &Locale,
    ) -> Result<()> {
        match self.channel_or_err(to.channel.as_str())?.editor() {
            Some(editor) => editor.edit(to, id, msg, locale).await,
            None => self.send(to, None, msg, locale).await.map(|_| ()),
        }
    }

    /// Runs one Job with its registered handler, outside any tick (no lock:
    /// `JobContext::progress` is a no-op). A panicking handler is an
    /// `Err` where unwinding is on (native).
    pub async fn run_job(&self, job: Job) -> Result<()> {
        self.run_claimed(job, None).await
    }

    pub(crate) async fn run_claimed(&self, job: Job, token: Option<&str>) -> Result<()> {
        let (_, h) = self
            .registry
            .jobs
            .get(job.kind.as_str())
            .ok_or_else(|| Error::Invalid(format!("no job kind {}", job.kind)))?;
        let cx = JobContext {
            services: self.services.clone(),
            catalog: self.catalog.clone(),
            config: self.config.clone(),
            jobs: self.queue.clone(),
            run: token.map(|t| (job.id.clone(), t.to_string())),
            // A shallow copy (every field is shared): Kernel Jobs that
            // Push (the roster import's revoked notices) need the Channels.
            deployment: Arc::new(self.clone()),
        };
        crate::jobs::catch::CatchPanic(h.run(cx, job)).await
    }
}

/// The handler of the longest prefix `id` starts with.
fn resolve_prefix(
    prefixes: &[(&'static str, &'static str, Arc<dyn ActionHandler>)],
    id: &str,
) -> Option<(&'static str, Arc<dyn ActionHandler>)> {
    prefixes
        .iter()
        .filter(|(p, _, _)| id.starts_with(p) && id.len() > p.len())
        .max_by_key(|(p, _, _)| p.len())
        .map(|(_, m, h)| (*m, h.clone()))
}

impl core::fmt::Debug for Deployment {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Deployment")
            .field("name", &self.config.name)
            .field(
                "channels",
                &self.channels.iter().map(|c| c.kind()).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::ActionContext;

    fn handler() -> Arc<dyn ActionHandler> {
        Arc::new(|_: ActionContext| async { Ok(Vec::new()) })
    }

    #[test]
    fn the_longest_prefix_wins_and_a_bare_prefix_matches_nothing() {
        let p = vec![("a.", "short", handler()), ("a.b.", "long", handler())];
        assert_eq!(resolve_prefix(&p, "a.b.c").map(|(m, _)| m), Some("long"));
        assert_eq!(resolve_prefix(&p, "a.x").map(|(m, _)| m), Some("short"));
        assert!(resolve_prefix(&p, "a.").is_none(), "no suffix");
        assert!(resolve_prefix(&p, "b.c").is_none());
    }
}
