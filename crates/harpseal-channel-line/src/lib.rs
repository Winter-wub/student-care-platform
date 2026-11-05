//! LINE adapter: type signatures only (prototype). Platform calls are
//! `todo!()`. Decisions that do not need the platform are real code.
//!
//! Capabilities: Mini App (LIFF). LINE cannot edit or delete a sent message.
//! Metering: Reply (reply token, short window) is free, Push is metered.

use harpseal::delivery::{
    ChannelAccountId, ChannelKind, Delivery, Inbound, IncomingFile, Metering, SendReceipt,
    WebhookRequest,
};
use harpseal::message::{Menu, Message};
use harpseal::Result;

pub const KIND: ChannelKind = ChannelKind("line");
/// Rendering rule: a Card List becomes a carousel of at most this many.
pub const CAROUSEL_MAX: usize = 12;
/// Rendering rule: Choices become quick replies, at most this many.
pub const QUICK_REPLY_MAX: usize = 13;

#[derive(Debug, Clone)]
pub struct LineConfig {
    pub channel_secret: String,
    pub access_token: String,
    pub liff_id: String,
}

/// LINE meters Push only.
pub fn metering(d: &Delivery) -> Metering {
    match d {
        Delivery::Reply(_) => Metering::Free,
        Delivery::Push => Metering::Metered,
    }
}

fn liff_url(cfg: &LineConfig, path: &str) -> String {
    format!("https://liff.line.me/{}{path}", cfg.liff_id)
}

use std::sync::Arc;


pub struct LineChannel {
    pub cfg: LineConfig,
    pub http: Arc<dyn harpseal::HttpClient>,
}

#[async_trait::async_trait]
impl harpseal::Channel for LineChannel {
    fn kind(&self) -> ChannelKind {
        KIND
    }
    async fn parse_webhook(&self, _req: WebhookRequest) -> Result<Vec<Inbound>> {
        todo!("check x-line-signature (HMAC-SHA256 of body with channel_secret), parse events")
    }
    async fn download(&self, _file: &IncomingFile) -> Result<Vec<u8>> {
        todo!("GET api-data.line.me/v2/bot/message/{{id}}/content via self.http")
    }
    async fn send(&self, _to: &ChannelAccountId, d: &Delivery, _m: &Message) -> Result<SendReceipt> {
        let _metering = metering(d);
        todo!("render to Flex / quick reply, POST reply or push via self.http")
    }
    async fn set_menu(&self, _to: &ChannelAccountId, _menu: Option<&Menu>) -> Result<()> {
        todo!("link per-user rich menu (None: unlink to the locked default)")
    }
    fn mini_app(&self) -> Option<&dyn harpseal::MiniApp> {
        Some(self)
    }
}

#[async_trait::async_trait]
impl harpseal::MiniApp for LineChannel {
    fn launch_url(&self, path: &str) -> String {
        liff_url(&self.cfg, path)
    }
    async fn verify_session(&self, _id_token: &str) -> Result<ChannelAccountId> {
        todo!("POST api.line.me/oauth2/v2.1/verify with the LIFF ID token")
    }
}

