//! Fake Channel: an in-memory Channel adapter for tests and demos. It
//! records everything the Core sends, so a test can read back what the
//! Student saw, and declares every Channel Capability (Mini App can be
//! switched off) so the Core's optional paths can be exercised.
//!
//! Metering mimics LINE: Reply is free, Push is metered.
//!
//! Webhooks: queue events with [`FakeChannel::push_inbound`], then POST
//! anything to `/webhook/fake` with header [`SIGNATURE_HEADER`] set to the
//! secret; the call drains the queue. A wrong or missing header is
//! rejected with `BadSignature` before the queue is touched.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use harpseal::{
    Attachment, Channel, ChannelAccountId, ChannelKind, DeleteMessage, Delivery, EditMessage,
    Error, Inbound, IncomingFile, Locale, Menu, Message, Metering, MiniApp, PlatformMessageId,
    Result, SendAttachment, SendReceipt, WebhookRequest,
};

/// The Fake Channel's kind.
pub const KIND: ChannelKind = ChannelKind("fake");
/// Header that must carry the secret on every webhook call.
pub const SIGNATURE_HEADER: &str = "x-fake-signature";
/// Base of the Fake Channel's Mini App URLs.
pub const MINI_APP_BASE: &str = "https://fake.invalid/app";

/// One message the Core sent through the Fake Channel.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Sent {
    /// Recipient.
    pub to: ChannelAccountId,
    /// Reply or Push, as the Core chose.
    pub delivery: Delivery,
    /// The message as the Core built it.
    pub message: Message,
    /// The id the Fake Channel gave it.
    pub id: PlatformMessageId,
    /// What the receipt reported.
    pub metering: Metering,
}

#[derive(Debug, Default)]
struct State {
    inbox: Vec<Inbound>,
    files: HashMap<String, Vec<u8>>,
    sent: Vec<Sent>,
    menus: HashMap<ChannelAccountId, Option<Menu>>,
    edited: Vec<(PlatformMessageId, Message)>,
    deleted: Vec<PlatformMessageId>,
}

/// Cloning shares the recorded state, so a test keeps a handle after
/// handing one to the Deployment builder.
#[derive(Debug, Clone)]
pub struct FakeChannel {
    secret: String,
    mini_app: bool,
    state: Arc<Mutex<State>>,
}

impl FakeChannel {
    /// A Fake Channel whose webhooks must carry `secret`. An empty secret
    /// is refused, so an unsigned call can never pass.
    pub fn new(secret: impl Into<String>) -> Result<Self> {
        let secret = secret.into();
        if secret.is_empty() {
            return Err(Error::Invalid(
                "the Fake Channel secret must not be empty".into(),
            ));
        }
        Ok(Self {
            secret,
            mini_app: true,
            state: Arc::default(),
        })
    }

    /// The same Channel without the Mini App Capability (as if no Mini App
    /// URL were configured).
    pub fn without_mini_app(mut self) -> Self {
        self.mini_app = false;
        self
    }

    /// The secret webhook calls must carry.
    pub fn secret(&self) -> &str {
        &self.secret
    }

    /// An account on this Channel.
    pub fn account(external_id: impl Into<String>) -> ChannelAccountId {
        ChannelAccountId::new(KIND, external_id)
    }

    /// Queues an event for the next webhook call.
    pub fn push_inbound(&self, ev: Inbound) {
        self.lock().inbox.push(ev);
    }

    /// Makes `bytes` downloadable as `platform_file_id`.
    pub fn add_file(&self, platform_file_id: impl Into<String>, bytes: Vec<u8>) {
        self.lock().files.insert(platform_file_id.into(), bytes);
    }

    /// Every message sent so far, in order.
    pub fn sent(&self) -> Vec<Sent> {
        self.lock().sent.clone()
    }

    /// The messages one account received, in order.
    pub fn sent_to(&self, to: &ChannelAccountId) -> Vec<Message> {
        let s = self.lock();
        s.sent
            .iter()
            .filter(|m| &m.to == to)
            .map(|m| m.message.clone())
            .collect()
    }

    /// The account's Menu: `None` if never set, `Some(None)` when locked.
    pub fn menu(&self, to: &ChannelAccountId) -> Option<Option<Menu>> {
        self.lock().menus.get(to).cloned()
    }

    /// Edits made through the EditMessage Capability.
    pub fn edited(&self) -> Vec<(PlatformMessageId, Message)> {
        self.lock().edited.clone()
    }

    /// Deletions made through the DeleteMessage Capability.
    pub fn deleted(&self) -> Vec<PlatformMessageId> {
        self.lock().deleted.clone()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn record(&self, to: &ChannelAccountId, d: &Delivery, m: Message) -> SendReceipt {
        let mut s = self.lock();
        let id = PlatformMessageId(format!("fake-{}", s.sent.len() + 1));
        let metering = match d {
            Delivery::Reply(_) => Metering::Free,
            _ => Metering::Metered,
        };
        s.sent.push(Sent {
            to: to.clone(),
            delivery: d.clone(),
            message: m,
            id: id.clone(),
            metering,
        });
        SendReceipt {
            message_id: Some(id),
            metering,
        }
    }
}

#[async_trait]
impl Channel for FakeChannel {
    fn kind(&self) -> ChannelKind {
        KIND
    }
    async fn parse_webhook(&self, req: WebhookRequest) -> Result<Vec<Inbound>> {
        let sig = req
            .headers
            .get(SIGNATURE_HEADER)
            .and_then(|v| v.to_str().ok());
        if self.secret.is_empty() || sig != Some(self.secret.as_str()) {
            return Err(Error::BadSignature);
        }
        Ok(std::mem::take(&mut self.lock().inbox))
    }
    async fn download(&self, file: &IncomingFile) -> Result<Vec<u8>> {
        self.lock()
            .files
            .get(&file.platform_file_id)
            .cloned()
            .ok_or_else(|| Error::Platform(format!("no fake file {}", file.platform_file_id)))
    }
    async fn send(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        m: &Message,
        _: &Locale,
    ) -> Result<SendReceipt> {
        Ok(self.record(to, d, m.clone()))
    }
    async fn set_menu(&self, to: &ChannelAccountId, menu: Option<&Menu>) -> Result<()> {
        self.lock().menus.insert(to.clone(), menu.cloned());
        Ok(())
    }
    fn editor(&self) -> Option<&dyn EditMessage> {
        Some(self)
    }
    fn deleter(&self) -> Option<&dyn DeleteMessage> {
        Some(self)
    }
    fn mini_app(&self) -> Option<&dyn MiniApp> {
        self.mini_app.then_some(self as &dyn MiniApp)
    }
    fn attachments(&self) -> Option<&dyn SendAttachment> {
        Some(self)
    }
}

#[async_trait]
impl EditMessage for FakeChannel {
    async fn edit(
        &self,
        _: &ChannelAccountId,
        id: &PlatformMessageId,
        m: &Message,
        _: &Locale,
    ) -> Result<()> {
        self.lock().edited.push((id.clone(), m.clone()));
        Ok(())
    }
}

#[async_trait]
impl DeleteMessage for FakeChannel {
    async fn delete(&self, _: &ChannelAccountId, id: &PlatformMessageId) -> Result<()> {
        self.lock().deleted.push(id.clone());
        Ok(())
    }
}

#[async_trait]
impl MiniApp for FakeChannel {
    fn launch_url(&self, path: &str) -> String {
        format!("{MINI_APP_BASE}{path}")
    }
    /// The token is the external id itself: tests open a Mini App page as
    /// any account by passing its id.
    async fn verify_session(&self, token: &str) -> Result<ChannelAccountId> {
        if token.is_empty() {
            return Err(Error::BadSignature);
        }
        Ok(Self::account(token))
    }
}

#[async_trait]
impl SendAttachment for FakeChannel {
    async fn send_attachment(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        file: &Attachment,
        _: &Locale,
    ) -> Result<SendReceipt> {
        Ok(self.record(to, d, Message::Attachment(file.clone())))
    }
}
