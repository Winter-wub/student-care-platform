//! A built Deployment: dispatches through the maps the builder validated.

mod builder;
mod web;

use std::collections::HashMap;
use std::sync::Arc;

pub use builder::DeploymentBuilder;

use crate::config::DeploymentConfig;
use crate::delivery::{
    ChannelAccountId, Delivery, Inbound, PlatformMessageId, ReplyToken, SendReceipt, WebhookRequest,
};
use crate::manifest::{Catalog, Job, JobSpec, MenuEntrySpec, Migration, Permission};
use crate::message::{ActionId, Locale, Menu, MenuEntry, Message};
use crate::migrate::MigrationReport;
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
    actions: HashMap<ActionId, Arc<dyn ActionHandler>>,
    jobs: HashMap<&'static str, (JobSpec, Arc<dyn JobHandler>)>,
    migrations: Vec<Migration>,
    back_office: Vec<(&'static str, &'static str, axum::Router<crate::AppState>)>,
    public: Vec<axum::Router<crate::AppState>>,
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

    /// Parses one webhook call for Channel `kind` and handles its events.
    pub async fn handle_webhook(&self, kind: &str, req: WebhookRequest) -> Result<()> {
        let channel = self.channel_or_err(kind)?;
        for ev in channel.parse_webhook(req).await? {
            self.handle_inbound(ev).await?;
        }
        Ok(())
    }

    /// Dispatches an Action from a linked account to its Module. The chat
    /// flow for unlinked accounts (Consent, Verification) arrives with #19;
    /// until then they get the `kernel.link.required` text.
    async fn handle_inbound(&self, ev: Inbound) -> Result<()> {
        let Inbound::Action {
            from,
            action,
            reply,
        } = ev
        else {
            return Ok(());
        };
        let rows = self.services.db.query(kernel::find_link(&from)).await?;
        let Some((student, locale)) = kernel::parse_link(&rows) else {
            let locale = self.config.default_locale.clone();
            let msg = Message::text(self.catalog.t(&locale, "kernel.link.required"));
            self.send(&from, reply.as_ref(), &msg, &locale).await?;
            return Ok(());
        };
        let Some(handler) = self.registry.actions.get(&action) else {
            let msg = Message::text(self.catalog.t(&locale, "kernel.action.expired"));
            self.send(&from, reply.as_ref(), &msg, &locale).await?;
            return Ok(());
        };
        let cx = ActionContext {
            services: self.services.clone(),
            catalog: self.catalog.clone(),
            student,
            account: from.clone(),
            locale: locale.clone(),
        };
        for msg in handler.handle(cx).await? {
            self.send(&from, reply.as_ref(), &msg, &locale).await?;
        }
        Ok(())
    }

    /// Picks Reply or Push ([`Delivery::select`]) and sends one message.
    /// The `outbound_message` row and `usage_daily` upsert arrive with #15.
    pub async fn send(
        &self,
        to: &ChannelAccountId,
        reply: Option<&ReplyToken>,
        msg: &Message,
        locale: &Locale,
    ) -> Result<SendReceipt> {
        let delivery = Delivery::select(reply, self.services.clock.now());
        self.channel_or_err(to.channel.as_str())?
            .send(to, &delivery, msg, locale)
            .await
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
