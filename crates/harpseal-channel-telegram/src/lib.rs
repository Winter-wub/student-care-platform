//! Telegram Bot API adapter.
//!
//! - **Webhook** ([`update`]): `X-Telegram-Bot-Api-Secret-Token` is compared
//!   in constant time before the body is read; message, callback_query and
//!   my_chat_member Updates become [`Inbound`] events. A pressed button is
//!   answered with `answerCallbackQuery` so the client stops spinning.
//! - **Commands as Actions**: Telegram sends a Menu command back as text
//!   (`/news_latest`). The adapter maps it to `Inbound::Action` through a
//!   command map: the per-account overlay written by each
//!   [`Channel::set_menu`] call, then the map given to
//!   [`TelegramChannel::with_menu`]. The overlay lives in memory, so on
//!   Workers it is per isolate; the `with_menu` map is the reliable source.
//!   Mapping a command for an account that has no Link is safe: the
//!   Kernel never dispatches an Action from an unlinked account to a
//!   Module (it answers `kernel.link.required`).
//! - **Sending** ([`render`]): per `docs/prototypes/message-render.md`.
//!   Every send is [`Metering::Free`] and, as Telegram has no reply tokens,
//!   a Push; the default [`Channel::send_batch`] (one `send` per Message)
//!   is right for Telegram.
//! - **Capabilities**: edit (`editMessageText`), delete (`deleteMessage`,
//!   only within 48 hours, else `Unsupported`), and Mini App (Web App)
//!   when a Web App URL is configured. Sending files is not offered yet.
//!
//! **Pacing.** Telegram asks for at most about 1 message per second per
//! chat and 30 per second overall
//! (<https://core.telegram.org/bots/faq#my-bot-is-hitting-limits-how-do-i-avoid-this>).
//! This adapter does not sleep (a Worker cannot wait usefully, and the Core
//! has no timer port): when Telegram answers 429 it returns
//! [`Error::RateLimited`] with Telegram's `retry_after`. Bulk sends are
//! paced by the Job runner (#23/#25), not here.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod api;
mod commands;
mod config;
pub mod render;
pub mod update;
pub mod web_app;

use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use harpseal::{
    ActionId, Capability, Channel, ChannelAccountId, ChannelKind, Clock, DeleteMessage, Delivery,
    EditMessage, Error, HttpClient, Inbound, IncomingFile, Locale, Menu, Message, Metering,
    MiniApp, PlatformMessageId, RenderCx, Result, SendReceipt, WebhookRequest,
};
use serde_json::{Value, json};

pub use api::{API_BASE, DOWNLOAD_MAX};
use commands::Commands;
pub use commands::command_map;
pub use config::TelegramConfig;
pub use update::{CommandMap, SECRET_HEADER};

/// Telegram's Channel kind.
pub const KIND: ChannelKind = ChannelKind("telegram");
/// `callback_data` is 1–64 bytes, which is where `ActionId::MAX_BYTES` comes from.
pub const CALLBACK_DATA_MAX: usize = harpseal::ActionId::MAX_BYTES;
/// The only command of the locked (pre-Link) Menu.
pub const START_COMMAND: &str = "start";
/// Its description in the command list.
pub const START_DESCRIPTION: &str = "เริ่มใช้งาน";

/// Telegram never meters.
pub fn metering(_: &Delivery) -> Metering {
    Metering::Free
}

/// One Bot API call: method, JSON body, and the FileStore key to upload as
/// multipart part "file" when the renderer chose `attach://file`.
#[derive(Debug, Clone)]
#[non_exhaustive]
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

/// The Telegram Channel. Clones share the command map, so a handle kept
/// after giving a clone to the Deployment builder can still call
/// [`TelegramChannel::with_menu`].
#[derive(Clone)]
pub struct TelegramChannel {
    cfg: TelegramConfig,
    http: Arc<dyn HttpClient>,
    clock: Option<Arc<dyn Clock>>,
    commands: Arc<Mutex<Commands>>,
}

impl TelegramChannel {
    /// A Telegram Channel calling the Bot API through `http`.
    pub fn new(cfg: TelegramConfig, http: Arc<dyn HttpClient>) -> Self {
        Self {
            cfg,
            http,
            clock: None,
            commands: Arc::default(),
        }
    }

    /// The Clock that judges how fresh Web App `initData` is (pass the
    /// Deployment's `services.clock`). Without it the Mini App Capability
    /// is absent, even with a Web App URL.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Sets the commands every account may type: pass the Deployment's
    /// unlocked Menu, `deployment.menu_for(&locale)`. Replaces any earlier
    /// map; shared with every clone.
    pub fn with_menu(&self, menu: &Menu) -> &Self {
        self.lock().base = command_map(menu);
        self
    }

    fn lock(&self) -> MutexGuard<'_, Commands> {
        self.commands.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The Action of command `cmd` for `from`: its `set_menu` overlay
    /// first, then the [`TelegramChannel::with_menu`] map.
    pub fn command_action(&self, from: &ChannelAccountId, cmd: &str) -> Option<ActionId> {
        let c = self.lock();
        c.overlay
            .get(&from.external_id)
            .and_then(|m| m.get(cmd))
            .or_else(|| c.base.get(cmd))
            .cloned()
    }

    /// The render context for one send.
    pub fn render_cx(&self, locale: &Locale) -> RenderCx {
        let mini_app_base_url = self.has_mini_app().then(|| self.cfg.web_app_url.clone());
        RenderCx {
            locale: locale.clone(),
            mini_app_base_url,
        }
    }

    /// A Web App URL and a Clock: the Mini App can open and be verified.
    fn has_mini_app(&self) -> bool {
        !self.cfg.web_app_url.is_empty() && self.clock.is_some()
    }

    fn api(&self) -> api::Api<'_> {
        api::Api {
            http: self.http.as_ref(),
            token: &self.cfg.bot_token,
        }
    }

    /// The menu button: the Web App when configured, else the command list.
    fn menu_button(&self) -> Value {
        if !self.has_mini_app() {
            json!({ "type": "commands" })
        } else {
            json!({ "type": "web_app", "text": self.cfg.menu_button_text,
                    "web_app": { "url": self.cfg.web_app_url } })
        }
    }
}

fn message_id(id: &PlatformMessageId) -> Result<i64> {
    id.0.parse()
        .map_err(|_| Error::Invalid(format!("not a Telegram message id: {}", id.0)))
}

#[async_trait]
impl Channel for TelegramChannel {
    fn kind(&self) -> ChannelKind {
        KIND
    }

    async fn parse_webhook(&self, req: WebhookRequest) -> Result<Vec<Inbound>> {
        update::verify(&req, &self.cfg.webhook_secret)?;
        let parsed = update::parse(&req.body, |from, cmd| self.command_action(from, cmd))?;
        if let Some(id) = parsed.answer_callback {
            // Best effort: a failed answer only leaves the spinner running.
            let body = json!({ "callback_query_id": id });
            let _ = self.api().call("answerCallbackQuery", &body).await;
        }
        if let Some(e) = parsed.refused {
            return Err(e);
        }
        Ok(parsed.event.into_iter().collect())
    }

    async fn download(&self, file: &IncomingFile) -> Result<Vec<u8>> {
        self.api().download(file).await
    }

    async fn send(
        &self,
        to: &ChannelAccountId,
        _d: &Delivery,
        m: &Message,
        locale: &Locale,
    ) -> Result<SendReceipt> {
        if let Message::Attachment(_) = m {
            return Err(Error::Unsupported(Capability::SendAttachment));
        }
        let calls = calls(to, render::message(m, &self.render_cx(locale)));
        if calls.is_empty() {
            return Err(Error::Invalid(format!(
                "Telegram cannot render a {}",
                m.kind()
            )));
        }
        let mut last = None;
        for c in calls {
            if c.upload.is_some() {
                return Err(Error::Unsupported(Capability::SendAttachment));
            }
            let result = self.api().call(&c.method, &c.body).await?;
            last = api::message_id(&result).or(last);
        }
        Ok(SendReceipt::new(
            last.map(PlatformMessageId),
            Metering::Free,
            true,
        ))
    }

    async fn set_menu(&self, to: &ChannelAccountId, menu: Option<&Menu>) -> Result<()> {
        let chat = &to.external_id;
        let scope = json!({ "type": "chat", "chat_id": chat });
        let (commands, button, map) = match menu {
            Some(m) => {
                let v = render::menu(m);
                let commands = v["setMyCommands"]["commands"].clone();
                (commands, self.menu_button(), command_map(m))
            }
            None => (
                json!([{ "command": START_COMMAND, "description": START_DESCRIPTION }]),
                json!({ "type": "commands" }),
                CommandMap::new(),
            ),
        };
        let api = self.api();
        let body = json!({ "commands": commands, "scope": scope });
        api.call("setMyCommands", &body).await?;
        let body = json!({ "chat_id": chat, "menu_button": button });
        api.call("setChatMenuButton", &body).await?;
        self.lock().set(chat.clone(), map);
        Ok(())
    }

    fn editor(&self) -> Option<&dyn EditMessage> {
        Some(self)
    }
    fn deleter(&self) -> Option<&dyn DeleteMessage> {
        Some(self)
    }
    /// Present only with a Web App URL and a Clock
    /// ([`TelegramChannel::with_clock`]): without the Clock `initData`
    /// cannot be checked, so the Capability is missing (and `build()`
    /// refuses Mini App buttons) rather than failing at runtime.
    fn mini_app(&self) -> Option<&dyn MiniApp> {
        self.has_mini_app().then_some(self as &dyn MiniApp)
    }
}

#[async_trait]
impl EditMessage for TelegramChannel {
    /// `editMessageText`: `msg` must render as one Telegram text message
    /// (a Text up to 4096 chars, a Card or Choices with ≤ 100 buttons).
    async fn edit(
        &self,
        to: &ChannelAccountId,
        id: &PlatformMessageId,
        msg: &Message,
        locale: &Locale,
    ) -> Result<()> {
        let mut calls = calls(to, render::message(msg, &self.render_cx(locale)));
        let mut call = match (calls.pop(), calls.is_empty()) {
            (Some(c), true) if c.method == "sendMessage" => c,
            _ => {
                return Err(Error::Invalid(
                    "an edit must render as one Telegram text message".into(),
                ));
            }
        };
        call.body["message_id"] = json!(message_id(id)?);
        self.api().call("editMessageText", &call.body).await?;
        Ok(())
    }
}

#[async_trait]
impl DeleteMessage for TelegramChannel {
    /// `deleteMessage`. Bots may delete their messages for 48 hours only;
    /// Telegram's refusal becomes `Unsupported(DeleteMessage)`.
    async fn delete(&self, to: &ChannelAccountId, id: &PlatformMessageId) -> Result<()> {
        let body = json!({ "chat_id": to.external_id, "message_id": message_id(id)? });
        self.api().call("deleteMessage", &body).await?;
        Ok(())
    }
}

#[async_trait]
impl MiniApp for TelegramChannel {
    fn launch_url(&self, path: &str) -> String {
        format!("{}{path}", self.cfg.web_app_url)
    }
    /// Validates `Telegram.WebApp.initData` ([`web_app::verify_init_data`]).
    async fn verify_session(&self, init_data: &str) -> Result<ChannelAccountId> {
        let clock = self.clock.as_ref().ok_or_else(|| {
            Error::Platform("Telegram Mini App needs TelegramChannel::with_clock".into())
        })?;
        web_app::verify_init_data(init_data, &self.cfg.bot_token, clock.now())
    }
}
