//! LINE Messaging API adapter. Rendering ([`render`]) and request packing
//! ([`requests`]) are real; the HTTP calls, signature check and webhook
//! parsing arrive with #16 and until then return [`Error::Platform`].
//!
//! Capabilities: Mini App (LIFF) when a LIFF id is configured. LINE cannot
//! edit or delete a sent message. Metering: Reply is free, Push is metered.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod render;

use std::sync::Arc;

use async_trait::async_trait;
use harpseal::{
    Channel, ChannelAccountId, ChannelKind, Delivery, Error, HttpClient, Inbound, IncomingFile,
    Locale, Menu, Message, Metering, MiniApp, RenderCx, Result, SendReceipt, WebhookRequest,
};
use serde_json::{Value, json};

/// LINE's Channel kind.
pub const KIND: ChannelKind = ChannelKind("line");

/// Secrets and ids of one LINE Official Account. Read from the environment
/// or Worker secrets, never from TOML.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LineConfig {
    /// Channel secret, for `x-line-signature`.
    pub channel_secret: String,
    /// Long-lived channel access token.
    pub access_token: String,
    /// LIFF app id; empty when the Deployment has no Mini App on LINE.
    pub liff_id: String,
}

impl LineConfig {
    /// A configuration; pass an empty `liff_id` for no Mini App.
    pub fn new(
        channel_secret: impl Into<String>,
        access_token: impl Into<String>,
        liff_id: impl Into<String>,
    ) -> Self {
        Self {
            channel_secret: channel_secret.into(),
            access_token: access_token.into(),
            liff_id: liff_id.into(),
        }
    }
}

/// LINE meters Push only.
pub fn metering(d: &Delivery) -> Metering {
    match d {
        Delivery::Reply(_) => Metering::Free,
        _ => Metering::Metered,
    }
}

/// One HTTP call: endpoint path and JSON body.
pub type Request = (&'static str, Value);

/// Packs rendered message objects into requests of at most 5. A reply token
/// works once, so only the first 5 go as a free Reply; the rest are Pushes
/// (metered). Returns the requests and whether any of them is metered.
pub fn requests(to: &ChannelAccountId, d: &Delivery, msgs: Vec<Value>) -> (Vec<Request>, Metering) {
    let mut out = Vec::new();
    let mut metering = Metering::Free;
    for (i, chunk) in msgs.chunks(render::MESSAGES_PER_REQUEST).enumerate() {
        match d.reply_token() {
            Some(token) if i == 0 => out.push((
                "/v2/bot/message/reply",
                json!({ "replyToken": token.token, "messages": chunk }),
            )),
            _ => {
                metering = Metering::Metered;
                out.push((
                    "/v2/bot/message/push",
                    json!({ "to": to.external_id, "messages": chunk }),
                ))
            }
        }
    }
    (out, metering)
}

fn not_yet(what: &str) -> Error {
    Error::Platform(format!(
        "LINE {what} arrives with the LINE adapter ticket (#16)"
    ))
}

/// The LINE Channel.
pub struct LineChannel {
    cfg: LineConfig,
    #[allow(dead_code)] // used by the HTTP calls (#16)
    http: Arc<dyn HttpClient>,
}

impl LineChannel {
    /// A LINE Channel sending through `http`.
    pub fn new(cfg: LineConfig, http: Arc<dyn HttpClient>) -> Self {
        Self { cfg, http }
    }

    fn liff_url(&self, path: &str) -> String {
        format!("https://liff.line.me/{}{path}", self.cfg.liff_id)
    }

    /// The render context for one send.
    pub fn render_cx(&self, locale: &Locale) -> RenderCx {
        let mini_app_base_url = (!self.cfg.liff_id.is_empty()).then(|| self.liff_url(""));
        RenderCx {
            locale: locale.clone(),
            mini_app_base_url,
        }
    }
}

#[async_trait]
impl Channel for LineChannel {
    fn kind(&self) -> ChannelKind {
        KIND
    }
    async fn parse_webhook(&self, _req: WebhookRequest) -> Result<Vec<Inbound>> {
        Err(not_yet("webhook parsing"))
    }
    async fn download(&self, _file: &IncomingFile) -> Result<Vec<u8>> {
        Err(not_yet("content download"))
    }
    async fn send(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        m: &Message,
        locale: &Locale,
    ) -> Result<SendReceipt> {
        let (_calls, _metering) = requests(to, d, render::message(m, &self.render_cx(locale)));
        Err(not_yet("sending"))
    }
    async fn set_menu(&self, _to: &ChannelAccountId, menu: Option<&Menu>) -> Result<()> {
        let _rich_menu = menu.map(render::menu);
        Err(not_yet("rich menus"))
    }
    fn mini_app(&self) -> Option<&dyn MiniApp> {
        (!self.cfg.liff_id.is_empty()).then_some(self as &dyn MiniApp)
    }
}

#[async_trait]
impl MiniApp for LineChannel {
    fn launch_url(&self, path: &str) -> String {
        self.liff_url(path)
    }
    async fn verify_session(&self, _id_token: &str) -> Result<ChannelAccountId> {
        Err(not_yet("LIFF id token verification"))
    }
}
