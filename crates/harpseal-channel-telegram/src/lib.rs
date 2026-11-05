//! Telegram adapter (prototype). Platform calls are `todo!()`; rendering
//! (`render`) is real code.
//!
//! Capabilities: edit, delete, Mini App (Web App). Every send is free; no
//! reply token exists, so the Core always selects Push.

use harpseal::Result;
use harpseal::delivery::{
    ChannelAccountId, ChannelKind, Delivery, Inbound, IncomingFile, Metering, PlatformMessageId,
    SendReceipt, WebhookRequest,
};
use harpseal::message::{Locale, Menu, Message};
use harpseal::render::RenderCx;
use serde_json::{Value, json};

pub mod render;

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

/// One Bot API call: method, JSON body, and the FileStore key to upload as
/// multipart part "file" when the renderer chose `attach://file`.
pub struct Call {
    pub method: String,
    pub body: Value,
    pub upload: Option<String>,
}

/// Adds `chat_id` to rendered calls; moves `method` and the upload key out.
pub fn calls(to: &ChannelAccountId, rendered: Vec<Value>) -> Vec<Call> {
    rendered
        .into_iter()
        .map(|mut v| {
            let method = v["method"].as_str().unwrap_or("sendMessage").to_string();
            let mut upload = None;
            if let Some(o) = v.as_object_mut() {
                o.remove("method");
                upload = o
                    .remove("x_upload_file_key")
                    .and_then(|k| k.as_str().map(String::from));
                o.insert("chat_id".into(), json!(to.external_id));
            }
            Call {
                method,
                body: v,
                upload,
            }
        })
        .collect()
}

use std::sync::Arc;

pub struct TelegramChannel {
    pub cfg: TelegramConfig,
    pub http: Arc<dyn harpseal::HttpClient>,
}

impl TelegramChannel {
    pub fn render_cx(&self, locale: &Locale) -> RenderCx {
        let mini_app_base_url =
            (!self.cfg.web_app_url.is_empty()).then(|| self.cfg.web_app_url.clone());
        RenderCx {
            locale: locale.clone(),
            mini_app_base_url,
        }
    }
}

#[async_trait::async_trait]
impl harpseal::Channel for TelegramChannel {
    fn kind(&self) -> ChannelKind {
        KIND
    }
    async fn parse_webhook(&self, _req: WebhookRequest) -> Result<Vec<Inbound>> {
        todo!("compare secret token header, parse Update (message / callback_query)")
    }
    async fn download(&self, _file: &IncomingFile) -> Result<Vec<u8>> {
        todo!("getFile then GET file path (20 MB bot API ceiling)")
    }
    async fn send(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        m: &Message,
        locale: &Locale,
    ) -> Result<SendReceipt> {
        let _ = metering(d);
        let calls = calls(to, render::message(m, &self.render_cx(locale)));
        let _ = calls;
        todo!(
            "POST api.telegram.org/bot<token>/<method> per call (≤ 1 msg/s per chat); upload x_upload_file_key as multipart"
        )
    }
    async fn set_menu(&self, _to: &ChannelAccountId, menu: Option<&Menu>) -> Result<()> {
        let _payloads = menu.map(render::menu);
        todo!("setMyCommands + setChatMenuButton with chat_id; None: deleteMyCommands scope=chat")
    }
    fn editor(&self) -> Option<&dyn harpseal::EditMessage> {
        Some(self)
    }
    fn deleter(&self) -> Option<&dyn harpseal::DeleteMessage> {
        Some(self)
    }
    fn mini_app(&self) -> Option<&dyn harpseal::MiniApp> {
        Some(self)
    }
}

#[async_trait::async_trait]
impl harpseal::EditMessage for TelegramChannel {
    async fn edit(
        &self,
        _: &ChannelAccountId,
        _: &PlatformMessageId,
        _: &Message,
        _: &Locale,
    ) -> Result<()> {
        todo!("editMessageText / editMessageReplyMarkup")
    }
}

#[async_trait::async_trait]
impl harpseal::DeleteMessage for TelegramChannel {
    async fn delete(&self, _: &ChannelAccountId, _: &PlatformMessageId) -> Result<()> {
        todo!("deleteMessage (only within 48 h)")
    }
}

#[async_trait::async_trait]
impl harpseal::MiniApp for TelegramChannel {
    fn launch_url(&self, path: &str) -> String {
        format!("{}{path}", self.cfg.web_app_url)
    }
    async fn verify_session(&self, _init_data: &str) -> Result<ChannelAccountId> {
        todo!("validate initData HMAC with the bot token")
    }
}
