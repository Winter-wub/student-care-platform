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

/// Design A: the HTTP client is a trait object from `Services`.
pub mod a {
    use std::sync::Arc;

    use super::*;
    use harpseal::design_a as a;

    pub struct LineChannel {
        pub cfg: LineConfig,
        pub http: Arc<dyn a::HttpClient>,
    }

    #[async_trait::async_trait]
    impl a::Channel for LineChannel {
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
        fn mini_app(&self) -> Option<&dyn a::MiniApp> {
            Some(self)
        }
    }

    #[async_trait::async_trait]
    impl a::MiniApp for LineChannel {
        fn launch_url(&self, path: &str) -> String {
            liff_url(&self.cfg, path)
        }
        async fn verify_session(&self, _id_token: &str) -> Result<ChannelAccountId> {
            todo!("POST api.line.me/oauth2/v2.1/verify with the LIFF ID token")
        }
    }
}

/// Design B: the adapter is generic over the runtime's HTTP client type.
pub mod b {
    use super::*;
    use harpseal::design_b as b;

    pub struct LineChannel<H: b::HttpClient> {
        pub cfg: LineConfig,
        pub http: H,
    }

    impl<H: b::HttpClient> b::Channel for LineChannel<H> {
        const KIND: ChannelKind = KIND;
        type Editor = b::Unsupported;
        type Deleter = b::Unsupported;
        type MiniApp = Self;

        async fn parse_webhook(&self, _req: WebhookRequest) -> Result<Vec<Inbound>> {
            todo!("check x-line-signature, parse events")
        }
        async fn download(&self, _file: &IncomingFile) -> Result<Vec<u8>> {
            todo!("GET message content via self.http")
        }
        async fn send(&self, _to: &ChannelAccountId, d: &Delivery, _m: &Message) -> Result<SendReceipt> {
            let _metering = metering(d);
            todo!("render and POST reply or push via self.http")
        }
        async fn set_menu(&self, _to: &ChannelAccountId, _menu: Option<&Menu>) -> Result<()> {
            todo!("link / unlink per-user rich menu")
        }
        fn mini_app(&self) -> Option<&Self> {
            Some(self)
        }
    }

    impl<H: b::HttpClient> b::MiniApp for LineChannel<H> {
        fn launch_url(&self, path: &str) -> String {
            liff_url(&self.cfg, path)
        }
        async fn verify_session(&self, _id_token: &str) -> Result<ChannelAccountId> {
            todo!("verify LIFF ID token")
        }
    }
}
