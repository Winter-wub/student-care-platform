//! Design A Deployment: a builder that collects boxed parts, validates them
//! at `build()`, and a Deployment that dispatches through maps.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;

use super::{
    ActionContext, ActionHandler, BackOfficeState, Channel, JobContext, JobHandler, Module,
    ModuleRegistrar, Services, VerificationMethod,
};
use crate::config::DeploymentConfig;
use crate::delivery::{ChannelAccountId, Delivery, Inbound, OutboundMessage, PlatformMessageId, ReplyToken, WebhookRequest};
use crate::manifest::{Catalog, Job, JobSpec, MenuEntrySpec, Migration, Permission};
use crate::message::{ActionId, Locale, Menu, MenuEntry, Message};
use crate::{BuildError, Error, Result, kernel};

pub struct Deployment {
    config: DeploymentConfig,
    services: Services,
    channels: Vec<Arc<dyn Channel>>,
    verification: Arc<dyn VerificationMethod>,
    catalog: Arc<Catalog>,
    permissions: Vec<Permission>,
    menu: Vec<MenuEntrySpec>,
    actions: HashMap<ActionId, Arc<dyn ActionHandler>>,
    jobs: HashMap<&'static str, (JobSpec, Arc<dyn JobHandler>)>,
    migrations: Vec<Migration>,
    back_office: Vec<(&'static str, axum::Router<super::AppState>)>,
}

#[derive(Default)]
pub struct DeploymentBuilder {
    config: Option<DeploymentConfig>,
    services: Option<Services>,
    channels: Vec<Arc<dyn Channel>>,
    modules: Vec<Box<dyn Module>>,
    verification: Option<Arc<dyn VerificationMethod>>,
}

impl Deployment {
    pub fn builder() -> DeploymentBuilder {
        DeploymentBuilder::default()
    }
}

impl DeploymentBuilder {
    pub fn config(mut self, c: DeploymentConfig) -> Self {
        self.config = Some(c);
        self
    }
    /// Plug a runtime: `harpseal_runtime_native::services(..)` etc.
    pub fn runtime(mut self, s: Services) -> Self {
        self.services = Some(s);
        self
    }
    pub fn channel(mut self, c: impl Channel + 'static) -> Self {
        self.channels.push(Arc::new(c));
        self
    }
    pub fn verification(mut self, v: impl VerificationMethod + 'static) -> Self {
        self.verification = Some(Arc::new(v));
        self
    }
    pub fn module(mut self, m: impl Module) -> Self {
        self.modules.push(Box::new(m));
        self
    }

    /// Runs every `Module::register`, merges, validates.
    pub fn build(self) -> Result<Deployment, BuildError> {
        let config = self.config.ok_or(BuildError::MissingConfig)?;
        let services = self.services.ok_or(BuildError::MissingRuntime)?;
        let verification = self.verification.ok_or(BuildError::MissingVerification)?;
        if self.channels.is_empty() {
            return Err(BuildError::NoChannel);
        }
        for (i, c) in self.channels.iter().enumerate() {
            if self.channels[..i].iter().any(|d| d.kind() == c.kind()) {
                return Err(BuildError::DuplicateChannel(c.kind().0));
            }
        }
        let mut catalog = Catalog::default();
        let (mut permissions, mut menu, mut migrations, mut back_office) = (vec![], vec![], vec![], vec![]);
        let (mut actions, mut jobs) = (HashMap::new(), HashMap::new());
        for m in &self.modules {
            let mut r = ModuleRegistrar::new(m.name());
            m.register(&mut r);
            for (id, h) in r.actions {
                if actions.insert(id.clone(), h).is_some() {
                    return Err(BuildError::DuplicateAction(id.as_str().into()));
                }
            }
            for (spec, h) in r.jobs {
                jobs.insert(spec.kind, (spec, h));
            }
            for e in r.catalog {
                catalog.insert(e.locale, e.key, e.text);
            }
            permissions.extend(r.permissions);
            menu.extend(r.menu);
            migrations.extend(r.migrations);
            for (perm, router) in r.back_office {
                if !permissions.iter().any(|p: &Permission| p.id == perm) {
                    return Err(BuildError::UnguardedRoute { module: r.module, permission: perm });
                }
                back_office.push((perm, router));
            }
        }
        kernel::check_permissions(&permissions, &config)?;
        config.apply_texts(&mut catalog);
        menu.sort_by_key(|e| e.order);
        Ok(Deployment {
            config,
            services,
            channels: self.channels,
            verification,
            catalog: Arc::new(catalog),
            permissions,
            menu,
            actions,
            jobs,
            migrations,
            back_office,
        })
    }
}

impl Deployment {
    pub fn config(&self) -> &DeploymentConfig {
        &self.config
    }
    pub fn permissions(&self) -> &[Permission] {
        &self.permissions
    }
    /// Kernel migrations first, then Modules in registration order.
    pub fn migrations(&self) -> &[Migration] {
        &self.migrations
    }

    fn channel(&self, kind: &str) -> Result<&Arc<dyn Channel>> {
        self.channels
            .iter()
            .find(|c| c.kind().0 == kind)
            .ok_or_else(|| Error::Invalid(format!("no channel {kind}")))
    }

    pub fn menu_for(&self, locale: &Locale) -> Menu {
        let entries = self.menu.iter().map(|e| MenuEntry {
            label: self.catalog.t(locale, e.label_key),
            action: e.action.clone(),
        });
        Menu { entries: entries.collect() }
    }

    /// Webhooks at `/webhook/{channel}`; Module pages under `/admin`.
    pub fn router(self: Arc<Self>) -> axum::Router {
        let state = Arc::new(BackOfficeState { services: self.services.clone(), catalog: self.catalog.clone() });
        let mut admin = axum::Router::new();
        for (_permission, r) in &self.back_office {
            // Prototype: the real Core wraps `r` in a layer that checks the
            // Staff session holds `_permission`.
            admin = admin.merge(r.clone());
        }
        let d = self.clone();
        let webhook = post(move |Path(kind): Path<String>, headers: HeaderMap, body: Bytes| {
            let d = d.clone();
            async move {
                let req = WebhookRequest { headers, body: body.to_vec() };
                match d.handle_webhook(&kind, req).await {
                    Ok(()) => StatusCode::OK,
                    Err(Error::BadSignature) => StatusCode::UNAUTHORIZED,
                    Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
                }
            }
        });
        axum::Router::new()
            .route("/webhook/{kind}", webhook)
            .nest("/admin", admin.with_state(state))
    }

    pub async fn handle_webhook(&self, kind: &str, req: WebhookRequest) -> Result<()> {
        let channel = self.channel(kind)?.clone();
        for ev in channel.parse_webhook(req).await? {
            self.handle_inbound(ev).await?;
        }
        Ok(())
    }

    async fn handle_inbound(&self, ev: Inbound) -> Result<()> {
        let Inbound::Action { from, action, reply } = ev else {
            return Ok(()); // text -> FAQ / Handoff, file -> Attachment: out of scope
        };
        let rows = self.services.db.query(kernel::find_link(&from)).await?;
        let Some((student, locale)) = kernel::parse_link(&rows) else {
            let _ = &self.verification; // Kernel would start Verification here
            let msg = Message::text(self.catalog.t(&self.config.default_locale, "link.required"));
            self.send(&from, reply.as_ref(), &msg).await?;
            return Ok(());
        };
        let handler = self.actions.get(&action).ok_or_else(|| Error::Invalid(action.as_str().into()))?;
        let cx = ActionContext { services: self.services.clone(), catalog: self.catalog.clone(), student, account: from.clone(), locale };
        for msg in handler.handle(cx).await? {
            self.send(&from, reply.as_ref(), &msg).await?;
        }
        Ok(())
    }

    /// Picks Reply or Push, sends, records the Outbound Message.
    pub async fn send(&self, to: &ChannelAccountId, reply: Option<&ReplyToken>, msg: &Message) -> Result<OutboundMessage> {
        let now = self.services.clock.now();
        let delivery = Delivery::select(reply, now);
        let receipt = self.channel(to.channel.0)?.send(to, &delivery, msg).await?;
        let out = OutboundMessage {
            to: to.clone(),
            push: matches!(delivery, Delivery::Push),
            metering: receipt.metering,
            message_id: receipt.message_id,
            sent_at: now,
        };
        self.services.db.execute(kernel::record_outbound(&out)).await?;
        Ok(out)
    }

    /// Uses the edit Capability when present, otherwise sends a new message.
    pub async fn update_message(&self, to: &ChannelAccountId, id: &PlatformMessageId, msg: &Message) -> Result<()> {
        match self.channel(to.channel.0)?.editor() {
            Some(editor) => editor.edit(to, id, msg).await,
            None => self.send(to, None, msg).await.map(|_| ()),
        }
    }

    pub async fn run_job(&self, job: Job) -> Result<()> {
        let (_, h) = self.jobs.get(job.kind.as_str()).ok_or_else(|| Error::Invalid(job.kind.clone()))?;
        h.run(JobContext { services: self.services.clone(), catalog: self.catalog.clone() }, job).await
    }
}
