//! Telegram Bot API adapter. Rendering ([`render`]) and call building
//! ([`calls`]) are real; the HTTP calls, secret-token check and update
//! parsing arrive with #17 and until then return [`Error::Platform`].
//!
//! Capabilities: edit, delete, and Mini App (Web App) when a Web App URL is
//! configured. Every send is free; there is no reply token, so the Core
//! always selects Push.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod render;

use std::sync::Arc;

use async_trait::async_trait;
use harpseal::{
    Channel, ChannelAccountId, ChannelKind, DeleteMessage, Delivery, EditMessage, Error,
    HttpClient, Inbound, IncomingFile, Locale, Menu, Message, Metering, MiniApp, PlatformMessageId,
    RenderCx, Result, SendReceipt, WebhookRequest,
};
use serde_json::{Value, json};

/// Telegram's Channel kind.
pub const KIND: ChannelKind = ChannelKind("telegram");
/// `callback_data` is 1–64 bytes, which is where `ActionId::MAX_BYTES` comes from.
pub const CALLBACK_DATA_MAX: usize = harpseal::ActionId::MAX_BYTES;

/// Secrets of one Telegram bot. Read from the environment or Worker
/// secrets, never from TOML.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TelegramConfig {
    /// Bot token from BotFather.
    pub bot_token: String,
    /// Checked against `X-Telegram-Bot-Api-Secret-Token`.
    pub webhook_secret: String,
    /// Web App URL; empty when the Deployment has no Mini App on Telegram.
    pub web_app_url: String,
}

impl TelegramConfig {
    /// A configuration; pass an empty `web_app_url` for no Mini App.
    pub fn new(
        bot_token: impl Into<String>,
        webhook_secret: impl Into<String>,
        web_app_url: impl Into<String>,
    ) -> Self {
        Self {
            bot_token: bot_token.into(),
            webhook_secret: webhook_secret.into(),
            web_app_url: web_app_url.into(),
        }
    }
}

/// Telegram never meters.
pub fn metering(_: &Delivery) -> Metering {
    Metering::Free
}

/// One Bot API call: method, JSON body, and the FileStore key to upload as
/// multipart part "file" when the renderer chose `attach://file`.
#[derive(Debug, Clone)]
pub struct Call {
    /// Bot API method, e.g. `sendMessage`.
    pub method: String,
    /// JSON parameters, with `chat_id`.
    pub body: Value,
    /// FileStore key to upload, if any.
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

fn not_yet(what: &str) -> Error {
    Error::Platform(format!(
        "Telegram {what} arrives with the Telegram adapter ticket (#17)"
    ))
}

/// The Telegram Channel.
pub struct TelegramChannel {
    cfg: TelegramConfig,
    #[allow(dead_code)] // used by the HTTP calls (#17)
    http: Arc<dyn HttpClient>,
}

impl TelegramChannel {
    /// A Telegram Channel calling the Bot API through `http`.
    pub fn new(cfg: TelegramConfig, http: Arc<dyn HttpClient>) -> Self {
        Self { cfg, http }
    }

    /// The render context for one send.
    pub fn render_cx(&self, locale: &Locale) -> RenderCx {
        let mini_app_base_url =
            (!self.cfg.web_app_url.is_empty()).then(|| self.cfg.web_app_url.clone());
        RenderCx {
            locale: locale.clone(),
            mini_app_base_url,
        }
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn kind(&self) -> ChannelKind {
        KIND
    }
    async fn parse_webhook(&self, _req: WebhookRequest) -> Result<Vec<Inbound>> {
        Err(not_yet("update parsing"))
    }
    async fn download(&self, _file: &IncomingFile) -> Result<Vec<u8>> {
        Err(not_yet("file download"))
    }
    async fn send(
        &self,
        to: &ChannelAccountId,
        _d: &Delivery,
        m: &Message,
        locale: &Locale,
    ) -> Result<SendReceipt> {
        let _calls = calls(to, render::message(m, &self.render_cx(locale)));
        Err(not_yet("sending"))
    }
    async fn set_menu(&self, _to: &ChannelAccountId, menu: Option<&Menu>) -> Result<()> {
        let _payloads = menu.map(render::menu);
        Err(not_yet("commands and menu button"))
    }
    fn editor(&self) -> Option<&dyn EditMessage> {
        Some(self)
    }
    fn deleter(&self) -> Option<&dyn DeleteMessage> {
        Some(self)
    }
    fn mini_app(&self) -> Option<&dyn MiniApp> {
        (!self.cfg.web_app_url.is_empty()).then_some(self as &dyn MiniApp)
    }
}

#[async_trait]
impl EditMessage for TelegramChannel {
    async fn edit(
        &self,
        _: &ChannelAccountId,
        _: &PlatformMessageId,
        _: &Message,
        _: &Locale,
    ) -> Result<()> {
        Err(not_yet("message editing"))
    }
}

#[async_trait]
impl DeleteMessage for TelegramChannel {
    async fn delete(&self, _: &ChannelAccountId, _: &PlatformMessageId) -> Result<()> {
        Err(not_yet("message deletion"))
    }
}

#[async_trait]
impl MiniApp for TelegramChannel {
    fn launch_url(&self, path: &str) -> String {
        format!("{}{path}", self.cfg.web_app_url)
    }
    async fn verify_session(&self, _init_data: &str) -> Result<ChannelAccountId> {
        Err(not_yet("Web App initData validation"))
    }
}
