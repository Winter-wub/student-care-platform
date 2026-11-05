//! Design B Deployment: a typestate builder. Each `.channel()` / `.module()`
//! grows a tuple type; `build()` exists only once a Verification Method is set.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;

use super::{
    ActionCx, BackOfficeState, Channel, ChannelSet, Clock, Db, JobCx, Manifest, Module, ModuleSet,
    Runtime, VerificationMethod,
};
use crate::config::DeploymentConfig;
use crate::delivery::{ChannelAccountId, Delivery, Inbound, OutboundMessage, PlatformMessageId, ReplyToken, WebhookRequest};
use crate::manifest::{Catalog, Job, Migration, Permission};
use crate::message::Message;
use crate::{BuildError, Error, Result, kernel};

/// `build()` is not implemented while the Verification Method is this.
pub struct NoVerification;

pub struct Builder<R, C, M, V> {
    runtime: R,
    channels: C,
    modules: M,
    verification: V,
}

pub struct Deployment<R: Runtime, C, M, V> {
    config: DeploymentConfig,
    runtime: Arc<R>,
    channels: C,
    modules: M,
    verification: V,
    catalog: Arc<Catalog>,
    manifest: Manifest<R>,
}

impl<R: Runtime> Deployment<R, (), (), NoVerification> {
    /// The runtime is required up front, so it can never be missing.
    pub fn builder(runtime: R) -> Builder<R, (), (), NoVerification> {
        Builder { runtime, channels: (), modules: (), verification: NoVerification }
    }
}

impl<R: Runtime, C, M, V> Builder<R, C, M, V> {
    pub fn channel<H: Channel>(self, ch: H) -> Builder<R, (H, C), M, V> {
        let Builder { runtime, channels, modules, verification } = self;
        Builder { runtime, channels: (ch, channels), modules, verification }
    }
    pub fn module<H: Module<R>>(self, m: H) -> Builder<R, C, (H, M), V> {
        let Builder { runtime, channels, modules, verification } = self;
        Builder { runtime, channels, modules: (m, modules), verification }
    }
    pub fn verification<V2: VerificationMethod<R>>(self, v: V2) -> Builder<R, C, M, V2> {
        let Builder { runtime, channels, modules, .. } = self;
        Builder { runtime, channels, modules, verification: v }
    }
}

impl<R: Runtime, C: ChannelSet, M: ModuleSet<R>, V: VerificationMethod<R>> Builder<R, C, M, V> {
    pub fn build(self, config: DeploymentConfig) -> Result<Deployment<R, C, M, V>, BuildError> {
        let mut kinds = Vec::new();
        self.channels.kinds(&mut kinds);
        if kinds.is_empty() {
            return Err(BuildError::NoChannel);
        }
        for (i, k) in kinds.iter().enumerate() {
            if kinds[..i].contains(k) {
                return Err(BuildError::DuplicateChannel(k.0));
            }
        }
        let mut manifest = Manifest::default();
        self.modules.collect(&mut manifest);
        for (i, e) in manifest.menu.iter().enumerate() {
            if manifest.menu[..i].iter().any(|f| f.action == e.action) {
                return Err(BuildError::DuplicateAction(e.action.as_str().into()));
            }
        }
        for (module, perm, _) in &manifest.back_office {
            if !manifest.permissions.iter().any(|p| p.id == *perm) {
                return Err(BuildError::UnguardedRoute { module, permission: perm });
            }
        }
        kernel::check_permissions(&manifest.permissions, &config)?;
        let mut catalog = Catalog::default();
        for e in &manifest.catalog {
            catalog.insert(e.locale, e.key, e.text);
        }
        config.apply_texts(&mut catalog);
        manifest.menu.sort_by_key(|e| e.order);
        Ok(Deployment {
            config,
            runtime: Arc::new(self.runtime),
            channels: self.channels,
            modules: self.modules,
            verification: self.verification,
            catalog: Arc::new(catalog),
            manifest,
        })
    }
}

impl<R: Runtime, C: ChannelSet, M: ModuleSet<R>, V: VerificationMethod<R>> Deployment<R, C, M, V> {
    pub fn config(&self) -> &DeploymentConfig {
        &self.config
    }
    /// The concrete runtime, e.g. to run migrations on its Db.
    pub fn runtime(&self) -> &R {
        &self.runtime
    }
    pub fn permissions(&self) -> &[Permission] {
        &self.manifest.permissions
    }
    pub fn migrations(&self) -> &[Migration] {
        &self.manifest.migrations
    }

    pub fn router(self: Arc<Self>) -> axum::Router {
        let state = Arc::new(BackOfficeState { runtime: self.runtime.clone(), catalog: self.catalog.clone() });
        let mut admin = axum::Router::new();
        for (_module, _permission, r) in &self.manifest.back_office {
            admin = admin.merge(r.clone()); // permission layer goes here
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
        let events = self.channels.parse_webhook(kind, req).await?
            .ok_or_else(|| Error::Invalid(format!("no channel {kind}")))?;
        for ev in events {
            self.handle_inbound(ev).await?;
        }
        Ok(())
    }

    async fn handle_inbound(&self, ev: Inbound) -> Result<()> {
        let Inbound::Action { from, action, reply } = ev else {
            return Ok(());
        };
        let rows = self.runtime.db().query(kernel::find_link(&from)).await?;
        let Some((student, locale)) = kernel::parse_link(&rows) else {
            let _ = (&self.verification, V::ID);
            let msg = Message::text(self.catalog.t(&self.config.default_locale, "link.required"));
            self.send(&from, reply.as_ref(), &msg).await?;
            return Ok(());
        };
        let cx = ActionCx { runtime: self.runtime.clone(), catalog: self.catalog.clone(), student, account: from.clone(), locale };
        let msgs = self.modules.on_action(&cx, &action).await?
            .ok_or_else(|| Error::Invalid(action.as_str().into()))?;
        for msg in msgs {
            self.send(&from, reply.as_ref(), &msg).await?;
        }
        Ok(())
    }

    pub async fn send(&self, to: &ChannelAccountId, reply: Option<&ReplyToken>, msg: &Message) -> Result<OutboundMessage> {
        let now = self.runtime.clock().now();
        let delivery = Delivery::select(reply, now);
        let receipt = self.channels.send(to, &delivery, msg).await?
            .ok_or_else(|| Error::Invalid(format!("no channel {}", to.channel.0)))?;
        let out = OutboundMessage {
            to: to.clone(),
            push: matches!(delivery, Delivery::Push),
            metering: receipt.metering,
            message_id: receipt.message_id,
            sent_at: now,
        };
        self.runtime.db().execute(kernel::record_outbound(&out)).await?;
        Ok(out)
    }

    pub async fn update_message(&self, to: &ChannelAccountId, id: &PlatformMessageId, msg: &Message) -> Result<()> {
        match self.channels.edit(to, id, msg).await {
            Err(Error::Unsupported(_)) => self.send(to, None, msg).await.map(|_| ()),
            other => other.map(|_| ()),
        }
    }

    pub async fn run_job(&self, job: Job) -> Result<()> {
        let cx = JobCx { runtime: self.runtime.clone(), catalog: self.catalog.clone() };
        match self.modules.run_job(&cx, &job).await? {
            true => Ok(()),
            false => Err(Error::Invalid(job.kind)),
        }
    }
}
