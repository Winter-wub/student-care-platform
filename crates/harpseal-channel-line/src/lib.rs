//! LINE adapter (prototype). Platform calls are `todo!()`; rendering
//! (`render`) and request packing (`requests`) are real code.
//!
//! Capabilities: Mini App (LIFF). LINE cannot edit or delete a sent message.
//! Metering: Reply (reply token, short window) is free, Push is metered.

use harpseal::Result;
use harpseal::delivery::{
    ChannelAccountId, ChannelKind, Delivery, Inbound, IncomingFile, Metering, SendReceipt,
    WebhookRequest,
};
use harpseal::message::{Locale, Menu, Message};
use harpseal::render::RenderCx;
use serde_json::{Value, json};

pub mod render;

pub const KIND: ChannelKind = ChannelKind("line");

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

/// One HTTP call: endpoint path and JSON body.
pub type Request = (&'static str, Value);

/// Packs rendered message objects into requests of at most 5. A reply token
/// works once, so only the first 5 go as a free Reply; the rest are Pushes
/// (metered). Returns the requests and whether any of them is metered.
pub fn requests(to: &ChannelAccountId, d: &Delivery, msgs: Vec<Value>) -> (Vec<Request>, Metering) {
    let mut out = Vec::new();
    let mut metering = Metering::Free;
    for (i, chunk) in msgs.chunks(render::MESSAGES_PER_REQUEST).enumerate() {
        match d {
            Delivery::Reply(token) if i == 0 => out.push((
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

use std::sync::Arc;

pub struct LineChannel {
    pub cfg: LineConfig,
    pub http: Arc<dyn harpseal::HttpClient>,
}

impl LineChannel {
    pub fn render_cx(&self, locale: &Locale) -> RenderCx {
        let mini_app_base_url = (!self.cfg.liff_id.is_empty()).then(|| liff_url(&self.cfg, ""));
        RenderCx {
            locale: locale.clone(),
            mini_app_base_url,
        }
    }
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
    async fn send(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        m: &Message,
        locale: &Locale,
    ) -> Result<SendReceipt> {
        let (calls, _metering) = requests(to, d, render::message(m, &self.render_cx(locale)));
        let _ = calls;
        todo!("POST each call to api.line.me via self.http with the access token")
    }
    async fn set_menu(&self, _to: &ChannelAccountId, menu: Option<&Menu>) -> Result<()> {
        let _rich_menu = menu.map(render::menu);
        todo!(
            "create rich menu (or reuse by hash), upload its image, link to the user; None: unlink"
        )
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
