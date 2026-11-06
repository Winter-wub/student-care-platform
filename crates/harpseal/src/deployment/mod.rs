//! A built Deployment: dispatches through the maps the builder validated.

mod builder;
mod routes;
mod send;
mod tick;
mod web;

use std::collections::HashMap;
use std::sync::Arc;

pub use builder::DeploymentBuilder;
pub use tick::TickReport;

use crate::config::DeploymentConfig;
use crate::delivery::{ChannelAccountId, Inbound, PlatformMessageId, WebhookRequest};
use crate::manifest::{Catalog, Job, JobSpec, MenuEntrySpec, Migration, Permission};
use crate::message::{ActionId, Locale, Menu, MenuEntry, Message};
use crate::migrate::MigrationReport;
use crate::port_types::Statement;
use crate::{
    ActionContext, ActionHandler, Channel, Error, JobContext, JobHandler, Result, Services,
    VerificationMethod, kernel,
};

/// One customer's running system: the Core, its configuration, Channels and
/// Modules. Build it with [`Deployment::builder`]; serve it with
/// [`Deployment::router`].
pub struct Deployment {
    config: Arc<DeploymentConfig>,
    services: Services,
    channels: Vec<Arc<dyn Channel>>,
    verification: Arc<dyn VerificationMethod>,
    catalog: Arc<Catalog>,
    registry: Registry,
}

/// What the Modules registered, after validation.
struct Registry {
    permissions: Vec<Permission>,
    menu: Vec<MenuEntrySpec>,
    actions: HashMap<ActionId, (&'static str, Arc<dyn ActionHandler>)>,
    jobs: HashMap<&'static str, (JobSpec, Arc<dyn JobHandler>)>,
    migrations: Vec<Migration>,
    /// (module, permission, path under `/admin`, handler).
    back_office: Vec<(&'static str, &'static str, &'static str, Route)>,
    /// (module, path, handler).
    public: Vec<(&'static str, &'static str, Route)>,
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
    /// Kernel migrations first, then Modules in registration order.
    pub fn migrations(&self) -> &[Migration] {
        &self.registry.migrations
    }

    /// Applies every pending migration to this Deployment's `Db`. Running
    /// it again is a no-op.
    pub async fn migrate(&self) -> Result<MigrationReport> {
        let now = self.services.now_ms();
        crate::migrate::migrate(self.services.db.as_ref(), self.migrations(), now).await
    }

    /// The Channel of `kind`.
    pub fn channel(&self, kind: &str) -> Option<&dyn Channel> {
        self.channels
            .iter()
            .find(|c| c.kind().as_str() == kind)
            .map(|c| c.as_ref())
    }

    fn channel_or_err(&self, kind: &str) -> Result<&dyn Channel> {
        self.channel(kind)
            .ok_or_else(|| Error::Invalid(format!("no channel {kind}")))
    }

    /// The unlocked Menu in `locale`.
    pub fn menu_for(&self, locale: &Locale) -> Menu {
        let entries = self.registry.menu.iter().map(|e| MenuEntry {
            label: self.catalog.t(locale, e.label_key),
            action: e.action.clone(),
        });
        Menu {
            entries: entries.collect(),
        }
    }

    /// Verifies and parses one webhook call for Channel `kind`, then
    /// handles each event on its own: a failing event is logged (Security
    /// Event `inbound.failed`) and does not stop the others. `Err` only when
    /// the call as a whole is refused: `BadSignature`, or the body could not
    /// be parsed.
    pub async fn handle_webhook(&self, kind: &str, req: WebhookRequest) -> Result<WebhookReport> {
        let channel = self.channel_or_err(kind)?;
        let events = channel.parse_webhook(req).await?;
        let mut report = WebhookReport {
            events: events.len(),
            failed: Vec::new(),
        };
        for ev in events {
            let ev_kind = ev.kind();
            let from = ev.from().clone();
            if let Err(e) = self.handle_inbound(ev).await {
                self.log_failed_event(&from, ev_kind, &e).await;
                report.failed.push((ev_kind, e));
            }
        }
        Ok(report)
    }

    /// Best effort: if even this write fails there is nowhere left to log.
    /// The account is linked when its `channel_account` row exists; the
    /// error text is cut to 200 chars.
    async fn log_failed_event(&self, from: &ChannelAccountId, event: &str, e: &Error) {
        let Ok(id) = self.services.new_id() else {
            return;
        };
        let error: String = e.to_string().chars().take(200).collect();
        let stmt = Statement::new(
            "INSERT INTO security_event (id, at, kind, channel_account_id, details) VALUES \
             (?, ?, 'inbound.failed', \
             (SELECT id FROM channel_account WHERE channel = ? AND external_id = ?), \
             json_object('channel', ?, 'event', ?, 'error', ?))",
        )
        .bind(id)
        .bind(self.services.now_ms())
        .bind(from.channel.as_str())
        .bind(from.external_id.clone())
        .bind(from.channel.as_str())
        .bind(event)
        .bind(error);
        let _ = self.services.db.execute(stmt).await;
    }

    /// Dispatches an Action from a linked account to its Module and sends
    /// its whole answer as one batch. The chat flow for unlinked accounts
    /// (Consent, Verification) arrives with #19; until then they get the
    /// `kernel.link.required` text.
    async fn handle_inbound(&self, ev: Inbound) -> Result<()> {
        let Inbound::Action {
            from,
            action,
            reply,
        } = ev
        else {
            return Ok(());
        };
        let reply = reply.as_ref();
        let kernel = Some(kernel::NAME);
        let rows = self.services.db.query(kernel::find_link(&from)).await?;
        let Some((student, locale)) = kernel::parse_link(&rows) else {
            let locale = self.config.default_locale.clone();
            let msg = Message::text(self.catalog.t(&locale, "kernel.link.required"));
            self.deliver(&from, reply, &[msg], &locale, kernel).await?;
            return Ok(());
        };
        let Some((module, handler)) = self.registry.actions.get(&action) else {
            let msg = Message::text(self.catalog.t(&locale, "kernel.action.expired"));
            self.deliver(&from, reply, &[msg], &locale, kernel).await?;
            return Ok(());
        };
        let cx = ActionContext {
            services: self.services.clone(),
            catalog: self.catalog.clone(),
            student,
            account: from.clone(),
            locale: locale.clone(),
        };
        let msgs = handler.handle(cx).await?;
        self.deliver(&from, reply, &msgs, &locale, Some(module))
            .await?;
        Ok(())
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

    /// Runs one Job with its registered handler.
    pub async fn run_job(&self, job: Job) -> Result<()> {
        let (_, h) = self
            .registry
            .jobs
            .get(job.kind.as_str())
            .ok_or_else(|| Error::Invalid(format!("no job kind {}", job.kind)))?;
        let cx = JobContext {
            services: self.services.clone(),
            catalog: self.catalog.clone(),
        };
        h.run(cx, job).await
    }
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
