//! Fake Channel: an in-memory Channel adapter for tests and demos. It
//! records every object the Core sends (degraded the way the chosen
//! [`FakeProfile`] would show it, with its Delivery and Metering), the
//! Menu per account, edits and deletes, so a test can read back what the
//! Student saw. [`FakeChannel::calls`] lists the platform requests (reply
//! or push, objects per request, metering) a real adapter would have made.
//!
//! Profiles: [`FakeProfile::Full`] (the default: every Capability, no
//! limits, Reply free and Push metered), [`FakeProfile::LineLike`] (a
//! single-use reply token covers 5 objects, then Push requests of 5, each
//! one metered unit; LINE's degrade limits, no attachments, no edit or
//! delete) and [`FakeProfile::TelegramLike`] (no
//! reply tokens, everything free, one message per Card, 100 buttons per
//! message).
//!
//! Webhooks: the body is a JSON array of events ([`FakeChannel::encode`])
//! and header [`SIGNATURE_HEADER`] carries [`FakeChannel::sign`] of it.
//! [`FakeChannel::webhook`] builds a signed call. A wrong or missing
//! signature is rejected with `BadSignature` before the body is read. An
//! event's `event_id` and `redelivery` travel as fields `id` and
//! `redelivery`, so a test can deliver the same event twice.
//!
//! Metering: a Message is charged one unit per metered Push request it
//! opens (a LineLike text split over 7 objects, sent by Push, opens 2
//! requests and costs 2). [`FakeChannel::fail_next_batch_at`] makes the
//! next batch stop part-way, to test partial sends.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod record;
mod render;
mod wire;

use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use harpseal::{
    BatchReceipt, Channel, ChannelAccountId, ChannelKind, DeleteMessage, Delivery, EditMessage,
    Error, Inbound, IncomingFile, Locale, Menu, Message, MiniApp, Result, SendAttachment,
    SendReceipt, WebhookRequest,
};

use record::State;
pub use record::{Call, FAILURE_RETRY_AFTER_SECS, Sent};
use render::Limits;

/// The Fake Channel's kind.
pub const KIND: ChannelKind = ChannelKind("fake");
/// Header that must carry [`FakeChannel::sign`] of the body.
pub const SIGNATURE_HEADER: &str = "x-fake-signature";
/// Base of the Fake Channel's Mini App URLs.
pub const MINI_APP_BASE: &str = "https://fake.invalid/app";

/// Which platform the Fake Channel imitates (metering, reply limit,
/// Capabilities and degrade rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FakeProfile {
    /// Every Capability, no limits; a reply token works once and covers
    /// its whole batch; one Push request per batch, metered.
    #[default]
    Full,
    /// LINE: a reply token works once and covers the first 5 objects of
    /// its batch; the rest go in Push requests of 5, each request one
    /// metered unit; text over 5000 chars split, over 12 Cards several
    /// carousels, over 13 quick replies (or a label over 20) one Card with
    /// every button; no attachments, edit or delete.
    LineLike,
    /// Telegram: no reply tokens, every send free, one message per Card,
    /// text over 4096 chars split, at most 100 buttons per message.
    TelegramLike,
}

impl FakeProfile {
    fn limits(self) -> Limits {
        match self {
            FakeProfile::Full => Limits::NONE,
            FakeProfile::LineLike => Limits::LINE,
            FakeProfile::TelegramLike => Limits::TELEGRAM,
        }
    }
    /// Objects one reply token covers (`None`: no limit).
    pub fn reply_limit(self) -> Option<usize> {
        match self {
            FakeProfile::LineLike => Some(5),
            _ => None,
        }
    }
    /// Most objects in one Push request: 5 on LINE, 1 on Telegram (one
    /// call per message), unlimited for `Full`.
    pub fn request_size(self) -> usize {
        match self {
            FakeProfile::Full => usize::MAX,
            FakeProfile::LineLike => 5,
            FakeProfile::TelegramLike => 1,
        }
    }
    /// Whether webhooks carry reply tokens.
    pub fn reply_tokens(self) -> bool {
        self != FakeProfile::TelegramLike
    }
    /// Whether a Push request counts against a Quota (one unit per request
    /// to one recipient, whatever the number of objects in it).
    pub fn push_metered(self) -> bool {
        self != FakeProfile::TelegramLike
    }
    fn attachments(self) -> bool {
        self != FakeProfile::LineLike
    }
    fn edit_delete(self) -> bool {
        self != FakeProfile::LineLike
    }
}

/// Cloning shares the recorded state, so a test keeps a handle after
/// handing one to the Deployment builder.
#[derive(Debug, Clone)]
pub struct FakeChannel {
    kind: ChannelKind,
    secret: String,
    mini_app: bool,
    profile: FakeProfile,
    state: Arc<Mutex<State>>,
}

impl FakeChannel {
    /// A [`FakeProfile::Full`] Fake Channel whose webhooks must be signed
    /// with `secret`. An empty secret is refused.
    pub fn new(secret: impl Into<String>) -> Result<Self> {
        let secret = secret.into();
        if secret.is_empty() {
            return Err(Error::Invalid(
                "the Fake Channel secret must not be empty".into(),
            ));
        }
        Ok(Self {
            kind: KIND,
            secret,
            mini_app: true,
            profile: FakeProfile::Full,
            state: Arc::default(),
        })
    }

    /// The same Channel imitating `profile` (recorded state is shared).
    pub fn with_profile(mut self, profile: FakeProfile) -> Self {
        self.profile = profile;
        self
    }
    /// [`FakeProfile::LineLike`].
    pub fn line_like(self) -> Self {
        self.with_profile(FakeProfile::LineLike)
    }
    /// [`FakeProfile::TelegramLike`].
    pub fn telegram_like(self) -> Self {
        self.with_profile(FakeProfile::TelegramLike)
    }
    /// The same Channel without the Mini App Capability (as if no Mini App
    /// URL were configured).
    pub fn without_mini_app(mut self) -> Self {
        self.mini_app = false;
        self
    }

    /// The same Channel under another kind (default [`KIND`]), so a test
    /// can register two Fake Channels in one Deployment. Its webhook path
    /// is `/webhook/<kind>`; recorded state is shared with `self`.
    pub fn with_kind(mut self, kind: &'static str) -> Self {
        self.kind = ChannelKind(kind);
        self
    }

    /// The profile imitated.
    pub fn profile(&self) -> FakeProfile {
        self.profile
    }
    /// The secret webhook calls are signed with.
    pub fn secret(&self) -> &str {
        &self.secret
    }
    /// An account on this Channel.
    pub fn account(external_id: impl Into<String>) -> ChannelAccountId {
        ChannelAccountId::new(KIND, external_id)
    }

    /// `events` in the webhook wire format (a JSON array). An Action's
    /// payload on the wire is its id, unchanged.
    pub fn encode(events: &[Inbound]) -> Vec<u8> {
        wire::encode(events)
    }
    /// The signature header value for `body`.
    pub fn sign(&self, body: &[u8]) -> String {
        wire::sign(&self.secret, body)
    }
    /// A correctly signed webhook call carrying `body` as is.
    pub fn signed(&self, body: Vec<u8>) -> WebhookRequest {
        let mut headers = http::HeaderMap::new();
        if let Ok(v) = http::HeaderValue::from_str(&self.sign(&body)) {
            headers.insert(SIGNATURE_HEADER, v);
        }
        WebhookRequest { headers, body }
    }
    /// A correctly signed webhook call carrying `events`.
    pub fn webhook(&self, events: &[Inbound]) -> WebhookRequest {
        self.signed(Self::encode(events))
    }

    /// Makes `bytes` downloadable as `platform_file_id`.
    pub fn add_file(&self, platform_file_id: impl Into<String>, bytes: Vec<u8>) {
        self.lock().files.insert(platform_file_id.into(), bytes);
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[async_trait]
impl Channel for FakeChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    async fn parse_webhook(&self, req: WebhookRequest) -> Result<Vec<Inbound>> {
        let sig = req
            .headers
            .get(SIGNATURE_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if sig.is_empty() || !wire::same(sig, &self.sign(&req.body)) {
            return Err(Error::BadSignature);
        }
        wire::decode(self.kind, &req.body, self.profile.reply_tokens())
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
        locale: &Locale,
    ) -> Result<SendReceipt> {
        let mut receipts = self
            .send_batch(to, d, std::slice::from_ref(m), locale)
            .await
            .into_result()?;
        receipts
            .pop()
            .ok_or_else(|| Error::Platform("fake: no receipt".into()))
    }
    async fn send_batch(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        msgs: &[Message],
        locale: &Locale,
    ) -> BatchReceipt {
        self.record_batch(to, d, msgs, locale)
    }
    async fn send_batch_keyed(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        msgs: &[Message],
        locale: &Locale,
        key: &str,
    ) -> BatchReceipt {
        self.lock().keys.push(key.to_string());
        self.record_batch(to, d, msgs, locale)
    }
    async fn set_menu(&self, to: &ChannelAccountId, menu: Option<&Menu>) -> Result<()> {
        let mut s = self.lock();
        if s.blocked.contains(to) {
            return Err(Error::Unreachable);
        }
        s.menus.insert(to.clone(), menu.cloned());
        Ok(())
    }
    fn editor(&self) -> Option<&dyn EditMessage> {
        self.profile
            .edit_delete()
            .then_some(self as &dyn EditMessage)
    }
    fn deleter(&self) -> Option<&dyn DeleteMessage> {
        self.profile
            .edit_delete()
            .then_some(self as &dyn DeleteMessage)
    }
    fn mini_app(&self) -> Option<&dyn MiniApp> {
        self.mini_app.then_some(self as &dyn MiniApp)
    }
    fn attachments(&self) -> Option<&dyn SendAttachment> {
        self.profile
            .attachments()
            .then_some(self as &dyn SendAttachment)
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
