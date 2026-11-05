//! Telegram adapter: type signatures only (prototype). Platform calls are
//! `todo!()`.
//!
//! Capabilities: edit, delete, Mini App (Web App). Every send is free; no
//! reply token exists, so the Core always selects Push.

use harpseal::delivery::{
    ChannelAccountId, ChannelKind, Delivery, Inbound, IncomingFile, Metering, PlatformMessageId,
    SendReceipt, WebhookRequest,
};
use harpseal::message::{Menu, Message};
use harpseal::Result;

pub const KIND: ChannelKind = ChannelKind("telegram");
/// `callback_data` is 1–64 bytes, which is where `ActionId::MAX_BYTES` comes from.
pub const CALLBACK_DATA_MAX: usize = harpseal::message::ActionId::MAX_BYTES;

#[derive(Debug, Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    /// Checked against `X-Telegram-Bot-Api-Secret-Token`.
    pub webhook_secret: String,
    pub web_app_url: String,
}

pub fn metering(_: &Delivery) -> Metering {
    Metering::Free
}

/// Design A.
pub mod a {
    use std::sync::Arc;

    use super::*;
    use harpseal::design_a as a;

    pub struct TelegramChannel {
        pub cfg: TelegramConfig,
        pub http: Arc<dyn a::HttpClient>,
    }

    #[async_trait::async_trait]
    impl a::Channel for TelegramChannel {
        fn kind(&self) -> ChannelKind {
            KIND
        }
        async fn parse_webhook(&self, _req: WebhookRequest) -> Result<Vec<Inbound>> {
            todo!("compare secret token header, parse Update (message / callback_query)")
        }
        async fn download(&self, _file: &IncomingFile) -> Result<Vec<u8>> {
            todo!("getFile then GET file path (20 MB bot API ceiling)")
        }
        async fn send(&self, _to: &ChannelAccountId, d: &Delivery, _m: &Message) -> Result<SendReceipt> {
            let _ = metering(d);
            todo!("sendMessage with inline_keyboard; Card List -> one message per Card")
        }
        async fn set_menu(&self, _to: &ChannelAccountId, _menu: Option<&Menu>) -> Result<()> {
            todo!("setMyCommands scope=chat + setChatMenuButton")
        }
        fn editor(&self) -> Option<&dyn a::EditMessage> {
            Some(self)
        }
        fn deleter(&self) -> Option<&dyn a::DeleteMessage> {
            Some(self)
        }
        fn mini_app(&self) -> Option<&dyn a::MiniApp> {
            Some(self)
        }
    }

    #[async_trait::async_trait]
    impl a::EditMessage for TelegramChannel {
        async fn edit(&self, _: &ChannelAccountId, _: &PlatformMessageId, _: &Message) -> Result<()> {
            todo!("editMessageText / editMessageReplyMarkup")
        }
    }

    #[async_trait::async_trait]
    impl a::DeleteMessage for TelegramChannel {
        async fn delete(&self, _: &ChannelAccountId, _: &PlatformMessageId) -> Result<()> {
            todo!("deleteMessage (only within 48 h)")
        }
    }

    #[async_trait::async_trait]
    impl a::MiniApp for TelegramChannel {
        fn launch_url(&self, path: &str) -> String {
            format!("{}{path}", self.cfg.web_app_url)
        }
        async fn verify_session(&self, _init_data: &str) -> Result<ChannelAccountId> {
            todo!("validate initData HMAC with the bot token")
        }
    }
}

/// Design B.
pub mod b {
    use super::*;
    use harpseal::design_b as b;

    pub struct TelegramChannel<H: b::HttpClient> {
        pub cfg: TelegramConfig,
        pub http: H,
    }

    impl<H: b::HttpClient> b::Channel for TelegramChannel<H> {
        const KIND: ChannelKind = KIND;
        type Editor = Self;
        type Deleter = Self;
        type MiniApp = Self;

        async fn parse_webhook(&self, _req: WebhookRequest) -> Result<Vec<Inbound>> {
            todo!("compare secret token header, parse Update")
        }
        async fn download(&self, _file: &IncomingFile) -> Result<Vec<u8>> {
            todo!("getFile then GET file path")
        }
        async fn send(&self, _to: &ChannelAccountId, d: &Delivery, _m: &Message) -> Result<SendReceipt> {
            let _ = metering(d);
            todo!("sendMessage with inline_keyboard")
        }
        async fn set_menu(&self, _to: &ChannelAccountId, _menu: Option<&Menu>) -> Result<()> {
            todo!("setMyCommands scope=chat + setChatMenuButton")
        }
        fn editor(&self) -> Option<&Self> {
            Some(self)
        }
        fn deleter(&self) -> Option<&Self> {
            Some(self)
        }
        fn mini_app(&self) -> Option<&Self> {
            Some(self)
        }
    }

    impl<H: b::HttpClient> b::EditMessage for TelegramChannel<H> {
        async fn edit(&self, _: &ChannelAccountId, _: &PlatformMessageId, _: &Message) -> Result<()> {
            todo!("editMessageText")
        }
    }

    impl<H: b::HttpClient> b::DeleteMessage for TelegramChannel<H> {
        async fn delete(&self, _: &ChannelAccountId, _: &PlatformMessageId) -> Result<()> {
            todo!("deleteMessage")
        }
    }

    impl<H: b::HttpClient> b::MiniApp for TelegramChannel<H> {
        fn launch_url(&self, path: &str) -> String {
            format!("{}{path}", self.cfg.web_app_url)
        }
        async fn verify_session(&self, _init_data: &str) -> Result<ChannelAccountId> {
            todo!("validate initData")
        }
    }
}
