//! LINE Messaging API adapter.
//!
//! - Webhooks ([`signature`], [`webhook`]): HMAC-SHA256 of the raw body
//!   checked against `x-line-signature` before any parsing; message,
//!   postback, follow and unfollow events become [`Inbound`]s.
//! - Files: downloaded from the content API through the `HttpClient` port.
//! - Sending ([`send`], [`render`]): Text, Card, CardList and Choices as
//!   Flex bubbles/carousels and quick replies, with the degrade rules of
//!   `docs/prototypes/message-render.md`. One batch spends the reply token
//!   on its first 5 objects (free); the rest go as Pushes of 5, each one
//!   metered unit. Blocks arrive as `unfollow` events (`Inbound::blocked`).
//!   A 403 on a send is `Error::Platform` (LINE documents it for a missing
//!   permission), except two undocumented per-user bodies kept as a
//!   defensive `Error::Unreachable` (see `docs/guide/channels.md`). 429 is
//!   `Error::RateLimited`.
//! - Menu ([`richmenu`]): one rich menu per Menu and image, reused by hash
//!   and linked per account; lower generations of a keyed Menu
//!   ([`harpseal::Menu::key`], [`LineConfig::menu_generation`]) are
//!   deleted. `set_menu(None)` links
//!   [`LineConfig::locked_menu`]; the Kernel is expected to pass its
//!   locked Menu (e.g. a "เริ่มใช้งาน" entry) through this config (#19).
//! - Capabilities: Mini App (LIFF, [`liff`]) when a LIFF id is configured;
//!   Multicast (500 recipients per request). LINE bots cannot edit or
//!   delete a sent message, and send no files (Attachment is
//!   `Unsupported`).

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod api;
pub mod liff;
pub mod multicast;
pub mod render;
mod resume;
pub mod retry_key;
pub mod richmenu;
pub mod send;
pub mod signature;
pub mod webhook;

use std::sync::Arc;

use async_trait::async_trait;
use harpseal::{
    BatchReceipt, Channel, ChannelAccountId, ChannelKind, Delivery, Error, HttpClient, Inbound,
    IncomingFile, Locale, Menu, Message, MiniApp, Multicast, RenderCx, Result, ResumeToken,
    SendReceipt, WebhookRequest,
};
use http::Method;

pub use api::{API, API_DATA, DEFAULT_RETRY_AFTER_SECS, RETRY_KEY_HEADER};
pub use liff::{LIFF_BASE, LINE_MINI_APP_BASE};
pub use send::metering;
pub use signature::{SIGNATURE_HEADER, sign};

/// LINE's Channel kind.
pub const KIND: ChannelKind = ChannelKind("line");

/// Secrets, ids and rich menu content of one LINE Official Account. The
/// secrets come from the environment or Worker secrets, never from TOML;
/// the images are loaded by the Deployment author.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct LineConfig {
    /// Channel secret, for `x-line-signature`.
    pub channel_secret: String,
    /// Long-lived channel access token.
    pub access_token: String,
    /// LIFF app id; empty when the Deployment has no Mini App on LINE.
    pub liff_id: String,
    /// Channel id of the LINE Login channel that owns the LIFF app, sent
    /// as `client_id` when verifying ID tokens. Empty: the part of
    /// `liff_id` before `-` (LIFF ids are `{loginChannelId}-{suffix}`).
    pub login_channel_id: String,
    /// Where launch URLs start, before the LIFF id. Empty: LIFF's
    /// `https://liff.line.me/`; a LINE MINI App uses
    /// [`LINE_MINI_APP_BASE`].
    pub launch_base: String,
    /// The locked Menu (before a Link), shown by `set_menu(None)`. Empty:
    /// `set_menu(None)` unlinks, showing the account's default rich menu.
    /// Its rich menu always has Menu key [`richmenu::LOCKED_KEY`] (any key
    /// on this Menu is ignored).
    pub locked_menu: Menu,
    /// PNG for the locked Menu's rich menu (see [`richmenu`] for the size).
    pub locked_menu_image: Vec<u8>,
    /// PNG for the unlocked Menu's rich menu.
    pub unlocked_menu_image: Vec<u8>,
    /// Generation of the rich menus this Deployment creates (0 counts as
    /// 1). Bump it when a Menu change must win: superseded rich menus are
    /// only those with a lower generation ([`richmenu`]).
    pub menu_generation: u32,
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
            menu_generation: 1,
            ..Self::default()
        }
    }

    /// The rich menu images: PNG bytes, 2500 x 1686 or 2500 x 843 px, at
    /// most 1 MB each
    /// (<https://developers.line.biz/en/reference/messaging-api/#upload-rich-menu-image-requirements>).
    pub fn with_menu_images(mut self, locked: Vec<u8>, unlocked: Vec<u8>) -> Self {
        self.locked_menu_image = locked;
        self.unlocked_menu_image = unlocked;
        self
    }

    /// The LINE Login channel id, when it is not the `liff_id` prefix.
    pub fn with_login_channel_id(mut self, id: impl Into<String>) -> Self {
        self.login_channel_id = id.into();
        self
    }

    /// Launch URLs under `base` instead of `https://liff.line.me/`, e.g.
    /// [`LINE_MINI_APP_BASE`] for a LINE MINI App (also a LIFF app: the
    /// page loads the same SDK with the same id).
    pub fn with_launch_base(mut self, base: impl Into<String>) -> Self {
        self.launch_base = base.into();
        self
    }

    /// The rich menu generation (default 1; 0 counts as 1). Bump it when a
    /// Menu change must replace the rich menus of the previous one; keep it
    /// when two versions may run on the channel at once.
    pub fn with_menu_generation(mut self, generation: u32) -> Self {
        self.menu_generation = generation;
        self
    }

    /// [`LineConfig::menu_generation`], at least 1.
    pub fn menu_generation(&self) -> u32 {
        self.menu_generation.max(1)
    }

    /// The locked Menu.
    pub fn with_locked_menu(mut self, menu: Menu) -> Self {
        self.locked_menu = menu;
        self
    }
}

impl core::fmt::Debug for LineConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LineConfig")
            .field("channel_secret", &"<redacted>")
            .field("access_token", &"<redacted>")
            .field("liff_id", &self.liff_id)
            .field("login_channel_id", &self.login_channel_id)
            .field("launch_base", &self.launch_base)
            .field("locked_menu", &self.locked_menu)
            .field("locked_menu_image", &self.locked_menu_image.len())
            .field("unlocked_menu_image", &self.unlocked_menu_image.len())
            .field("menu_generation", &self.menu_generation)
            .finish()
    }
}

/// The LINE Channel. Clones share the rich menu cache.
#[derive(Clone)]
pub struct LineChannel {
    cfg: LineConfig,
    http: Arc<dyn HttpClient>,
    menus: Arc<richmenu::MenuCache>,
}

impl LineChannel {
    /// A LINE Channel sending through `http`.
    pub fn new(cfg: LineConfig, http: Arc<dyn HttpClient>) -> Self {
        Self {
            cfg,
            http,
            menus: Default::default(),
        }
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
    async fn parse_webhook(&self, req: WebhookRequest) -> Result<Vec<Inbound>> {
        signature::verify(&self.cfg.channel_secret, &req.headers, &req.body)?;
        webhook::parse(&req.body)
    }
    /// From the content API only
    /// (<https://developers.line.biz/en/reference/messaging-api/#get-content>).
    /// Any status but 200 is an error: video and audio answer otherwise
    /// while LINE is still preparing them. Content held by an external
    /// provider is refused; the adapter never fetches a URL a sender chose.
    async fn download(&self, file: &IncomingFile) -> Result<Vec<u8>> {
        let id = &file.platform_file_id;
        if id.starts_with(webhook::EXTERNAL) {
            return Err(Error::Platform(format!(
                "LINE content {id} is held by an external provider; only the content API is used"
            )));
        }
        let url = format!("{API_DATA}/v2/bot/message/{}/content", api::path_id(id)?);
        let resp = self
            .send_raw(Method::GET, &url, None, Vec::new(), true)
            .await?;
        match resp.status().as_u16() {
            200 => Ok(resp.into_body()),
            s if (201..300).contains(&s) => Err(Error::Platform(format!(
                "LINE content {id} is not ready yet ({s})"
            ))),
            _ => Err(api::answer(resp).err().map_or_else(
                || Error::Platform("LINE content download failed".into()),
                |f| f.into_error(false),
            )),
        }
    }
    async fn send(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        m: &Message,
        locale: &Locale,
    ) -> Result<SendReceipt> {
        let mut receipts = self
            .batch(to, d, std::slice::from_ref(m), locale, None, None)
            .await
            .into_result()?;
        receipts
            .pop()
            .ok_or_else(|| Error::Platform("LINE: no receipt".into()))
    }
    async fn send_batch(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        msgs: &[Message],
        locale: &Locale,
    ) -> BatchReceipt {
        self.batch(to, d, msgs, locale, None, None).await
    }
    async fn send_batch_keyed(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        msgs: &[Message],
        locale: &Locale,
        key: &str,
    ) -> BatchReceipt {
        self.batch(to, d, msgs, locale, Some(key), None).await
    }
    async fn send_batch_resumed(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        msgs: &[Message],
        locale: &Locale,
        key: &str,
        resume: Option<&ResumeToken>,
    ) -> BatchReceipt {
        self.batch(to, d, msgs, locale, Some(key), resume).await
    }
    async fn set_menu(&self, to: &ChannelAccountId, menu: Option<&Menu>) -> Result<()> {
        self.rich_menu_set(to, menu).await
    }
    fn mini_app(&self) -> Option<&dyn MiniApp> {
        (!self.cfg.liff_id.is_empty()).then_some(self as &dyn MiniApp)
    }
    fn multicast(&self) -> Option<&dyn Multicast> {
        Some(self)
    }
}
