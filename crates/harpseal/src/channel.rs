//! Channel: one object-safe trait; optional Capabilities are accessor
//! methods returning `Option<&dyn _>`, so the declaration and the behaviour
//! cannot disagree.

use async_trait::async_trait;

use crate::delivery::{
    BatchReceipt, Capabilities, Capability, ChannelAccountId, ChannelKind, Delivery, Inbound,
    IncomingFile, PlatformMessageId, SendReceipt, WebhookRequest,
};
use crate::message::{Attachment, Locale, Menu, Message};
use crate::{Error, Result};

/// A messaging platform adapter. The required minimum: receive text and
/// files (`parse_webhook` + `download`), send Text/Card/CardList/Choices
/// (`send`), set the Menu per Channel Account (`set_menu`) and report
/// metered/free per message ([`SendReceipt`]).
#[async_trait]
pub trait Channel: Send + Sync {
    /// Unique per Deployment. A method, not a const: consts break `dyn`.
    fn kind(&self) -> ChannelKind;

    /// Verifies the signature and turns one webhook call into events.
    /// `Err(BadSignature)` must be returned before any parsing.
    async fn parse_webhook(&self, req: WebhookRequest) -> Result<Vec<Inbound>>;

    /// Fetches the bytes of a file a Student sent so the Core can copy it.
    async fn download(&self, file: &IncomingFile) -> Result<Vec<u8>>;

    /// Renders and sends one message. The adapter degrades what it cannot
    /// show (never dropping an Action) and reports whether the platform
    /// metered it. `locale` is the Student's, for degrade texts.
    async fn send(
        &self,
        to: &ChannelAccountId,
        delivery: &Delivery,
        msg: &Message,
        locale: &Locale,
    ) -> Result<SendReceipt>;

    /// Sends a handler's whole batch to one account with one Delivery. The
    /// Core calls this, never [`Channel::send`] directly, so an adapter
    /// whose reply token is single-use or limited (LINE: one reply call of
    /// at most 5 objects) can spend the token on the first objects and send
    /// the rest as Pushes, reporting that in each [`SendReceipt`].
    ///
    /// The default treats a reply token as single-use: the first Message
    /// goes with the given Delivery and every later one as
    /// [`Delivery::Push`]. A Channel without reply tokens (Telegram) is
    /// unaffected; an adapter that can put several objects in one reply
    /// (LINE) must override this, or the rest of each batch costs metered
    /// Pushes (never a lost message). It routes [`Message::Attachment`]
    /// through [`Channel::attachments`], or stops with
    /// `Error::Unsupported(SendAttachment)` when that Capability is absent,
    /// and stops at the first error. An adapter that splits one Message
    /// into several platform messages should override this to report the
    /// parts that went out before a failure ([`BatchReceipt::part_way`]).
    async fn send_batch(
        &self,
        to: &ChannelAccountId,
        delivery: &Delivery,
        msgs: &[Message],
        locale: &Locale,
    ) -> BatchReceipt {
        let mut receipts = Vec::with_capacity(msgs.len());
        for (i, msg) in msgs.iter().enumerate() {
            let d = if i == 0 { delivery } else { &Delivery::Push };
            let sent = match msg {
                Message::Attachment(file) => match self.attachments() {
                    Some(a) => a.send_attachment(to, d, file, locale).await,
                    None => Err(Error::Unsupported(Capability::SendAttachment)),
                },
                _ => self.send(to, d, msg, locale).await,
            };
            match sent {
                Ok(r) => receipts.push(r),
                Err(e) => return BatchReceipt::partial(receipts, e),
            }
        }
        BatchReceipt::complete(receipts)
    }

    /// [`Channel::send_batch`] with an idempotency `key` the Core chose for
    /// this batch: the same key, account and Messages on a retry mean the
    /// same send, so an adapter whose platform de-duplicates requests
    /// (LINE `X-Line-Retry-Key`) derives each request's key from it and a
    /// request that already went out is not delivered twice. The Core
    /// calls this; the default ignores the key.
    async fn send_batch_keyed(
        &self,
        to: &ChannelAccountId,
        delivery: &Delivery,
        msgs: &[Message],
        locale: &Locale,
        key: &str,
    ) -> BatchReceipt {
        let _ = key;
        self.send_batch(to, delivery, msgs, locale).await
    }

    /// [`Channel::send_batch_keyed`] resuming a batch that stopped part-way
    /// with `resume`, the token this adapter put in that attempt's
    /// [`BatchReceipt::resume`]. `msgs` are the Messages from the one the
    /// last attempt stopped in on (`msgs[sent..]` of the batch). An adapter
    /// that packs parts of several Messages into one request (LINE) uses
    /// the token to rebuild the requests the failed attempt would have
    /// made from the failing one on, with the same retry keys, so a part
    /// already accepted is answered as a replay and never reaches the
    /// Student twice. The default ignores the token: the resume starts at
    /// the first part of `msgs[0]`, which is right for an adapter that
    /// sends one part per request and has no retry keys (the parts of
    /// `msgs[0]` already delivered go again).
    async fn send_batch_resumed(
        &self,
        to: &ChannelAccountId,
        delivery: &Delivery,
        msgs: &[Message],
        locale: &Locale,
        key: &str,
        resume: Option<&ResumeToken>,
    ) -> BatchReceipt {
        let _ = resume;
        self.send_batch_keyed(to, delivery, msgs, locale, key).await
    }

    /// Sets the Menu for one account; `None` resets it to the locked
    /// pre-Link Menu.
    async fn set_menu(&self, to: &ChannelAccountId, menu: Option<&Menu>) -> Result<()>;

    /// Most accounts the Kernel hands [`Channel::relink_menu`] in one Job
    /// run. The default (20) keeps an adapter with two platform calls per
    /// account under 50 subrequests per Job run (the Cloudflare Workers
    /// free plan's per-invocation limit); one with a bulk call (LINE)
    /// raises it.
    fn relink_chunk(&self) -> usize {
        20
    }

    /// Shows `menu`, a newer revision of its key ([`Menu::revision`]), to
    /// every account in `to`: the Kernel's Menu-label re-link after a
    /// back-office save. Accounts are processed in order. An account the
    /// platform will never reach ([`Error::Unreachable`]: blocked, deleted)
    /// or whose id it refuses is listed in [`MenuRelinked::skipped`] and
    /// passed over; any other error stops the call, and
    /// [`MenuRelinked::done`] says how many from the start are done (the
    /// Kernel resumes at `to[done..]`). An adapter may also stop early
    /// without an error (a call budget spent): the Kernel then goes on at
    /// `to[done..]` in its next run. The default calls
    /// [`Channel::set_menu`] per account.
    async fn relink_menu(&self, to: &[ChannelAccountId], menu: &Menu) -> MenuRelinked {
        let mut got = MenuRelinked::all(to.len());
        for (i, account) in to.iter().enumerate() {
            match self.set_menu(account, Some(menu)).await {
                Ok(()) => {}
                Err(e) if matches!(e.root(), Error::Unreachable) => got.skip(i, e),
                Err(e) => {
                    got.done = i;
                    got.error = Some(e);
                    return got;
                }
            }
        }
        got
    }

    /// Retires the platform objects of the `previous` revisions of
    /// `menu`'s key, once every account was re-linked to `menu`. Called
    /// by the Kernel only after [`Channel::relink_menu`] finished for
    /// every linked account. An adapter must touch nothing but the
    /// versions named by `previous` (never another key, another
    /// Deployment's version or a revision not listed). When the platform
    /// works asynchronously, the adapter answers [`MenuRetire::Pending`]
    /// with a token, and the Kernel calls again with it later; a version
    /// must not be deleted before the platform confirmed nobody is left
    /// on it. The default retires nothing (an adapter whose Menu is no
    /// platform object, e.g. per-chat commands).
    async fn retire_menus(
        &self,
        menu: &Menu,
        previous: &[Menu],
        pending: Option<&str>,
    ) -> Result<MenuRetire> {
        let _ = (menu, previous, pending);
        Ok(MenuRetire::Done)
    }

    /// Channel Capability: edit a sent message.
    fn editor(&self) -> Option<&dyn EditMessage> {
        None
    }
    /// Channel Capability: delete a sent message.
    fn deleter(&self) -> Option<&dyn DeleteMessage> {
        None
    }
    /// Channel Capability: Mini App (LIFF / Telegram Web App). `None` when
    /// the adapter has no Mini App URL configured.
    fn mini_app(&self) -> Option<&dyn MiniApp> {
        None
    }
    /// Channel Capability: send a file to a Student.
    fn attachments(&self) -> Option<&dyn SendAttachment> {
        None
    }
    /// Channel Capability: one request to many accounts (LINE multicast),
    /// for Announcements. `None` (the default): send to each account.
    fn multicast(&self) -> Option<&dyn Multicast> {
        None
    }

    /// Derived from the accessors above.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            edit_message: self.editor().is_some(),
            delete_message: self.deleter().is_some(),
            mini_app: self.mini_app().is_some(),
            send_attachment: self.attachments().is_some(),
            multicast: self.multicast().is_some(),
        }
    }
}

/// Capability: edit a message already sent.
#[async_trait]
pub trait EditMessage: Send + Sync {
    /// Replaces message `id` with `msg`.
    async fn edit(
        &self,
        to: &ChannelAccountId,
        id: &PlatformMessageId,
        msg: &Message,
        locale: &Locale,
    ) -> Result<()>;
}

/// Capability: delete a message already sent.
#[async_trait]
pub trait DeleteMessage: Send + Sync {
    /// Deletes message `id`.
    async fn delete(&self, to: &ChannelAccountId, id: &PlatformMessageId) -> Result<()>;
}

/// Capability: Mini App pages inside the chat app.
#[async_trait]
pub trait MiniApp: Send + Sync {
    /// URL that opens `path` inside the chat app.
    fn launch_url(&self, path: &str) -> String;
    /// Resolves the platform's signed token to a Channel Account (App
    /// Session).
    async fn verify_session(&self, signed_token: &str) -> Result<ChannelAccountId>;
    /// The platform script the Mini App page loads to read the signed
    /// token in the webview. `None` (the default): no script; the token
    /// is posted some other way (the Fake Channel's tests post it).
    fn web_client(&self) -> Option<WebClient> {
        None
    }
}

/// The platform's web SDK a Mini App page loads, as a [`MiniApp`] reports
/// it. The Kernel's entry page loads the script and reads the token with
/// it; the URL of the Mini App plays no part.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WebClient {
    /// LINE's LIFF SDK, for a LIFF app or a LINE MINI App
    /// (`liff.init({ liffId: id })`, then `liff.getIDToken()`).
    Liff {
        /// The LIFF id.
        id: String,
    },
    /// Telegram's Web App script (`Telegram.WebApp.initData`).
    Telegram,
}

/// Capability: send a file.
#[async_trait]
pub trait SendAttachment: Send + Sync {
    /// Sends `file` as its own message.
    async fn send_attachment(
        &self,
        to: &ChannelAccountId,
        delivery: &Delivery,
        file: &Attachment,
        locale: &Locale,
    ) -> Result<SendReceipt>;
}

/// Capability: send the same Messages to many accounts with one request per
/// group of recipients (LINE: at most 500 per multicast request). Always a
/// Push: there is no reply token.
#[async_trait]
pub trait Multicast: Send + Sync {
    /// Most recipients one platform request takes; the adapter splits a
    /// longer `to` itself.
    fn max_recipients(&self) -> usize;
    /// Sends `msgs` to every account in `to`, with idempotency `key` (as
    /// in [`Channel::send_batch_keyed`]: the same key, recipients and
    /// Messages on a retry mean the same send). The receipts are
    /// recipient-major: `receipts[i * msgs.len() + j]` is Message `j` to
    /// `to[i]`. Metering is per recipient: for each recipient, the receipt
    /// of the Message that opens a platform request gets one metered unit
    /// per request it opens. When a request fails, the receipts cover the
    /// recipients whose every request went out (a whole number of groups
    /// of [`Multicast::max_recipients`]) and `error` is set: a retry
    /// resumes with `to[receipts.len() / msgs.len()..]` and the same key.
    async fn send_many(
        &self,
        to: &[ChannelAccountId],
        msgs: &[Message],
        locale: &Locale,
        key: &str,
    ) -> BatchReceipt;
}

/// What [`Channel::relink_menu`] got through.
#[derive(Debug)]
#[non_exhaustive]
pub struct MenuRelinked {
    /// Accounts done, counted from the start of `to` (skipped ones
    /// included).
    pub done: usize,
    /// Why it stopped before the end, if it did.
    pub error: Option<Error>,
    /// Accounts among the first `done` that cannot show the Menu for good
    /// (index in `to`, and why): [`Error::Unreachable`] (the Kernel marks
    /// the Link unreachable) or an id the platform refuses
    /// ([`Error::Invalid`]). They do not stop the re-link.
    pub skipped: Vec<(usize, Error)>,
}

impl MenuRelinked {
    /// Every one of `done` accounts.
    pub fn all(done: usize) -> Self {
        Self {
            done,
            error: None,
            skipped: Vec::new(),
        }
    }
    /// Stopped by `error` after `done` accounts.
    pub fn stopped(done: usize, error: Error) -> Self {
        Self {
            done,
            error: Some(error),
            skipped: Vec::new(),
        }
    }
    /// Records account `index` as skipped for good, because of `error`.
    pub fn skip(&mut self, index: usize, error: Error) {
        self.skipped.push((index, error));
    }
}

/// What [`Channel::retire_menus`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MenuRetire {
    /// The previous revisions are gone (or never existed).
    Done,
    /// The platform is still moving accounts off them: call again after
    /// `retry_after_secs` with `token`.
    Pending {
        /// The adapter's own encoding of what it is waiting for.
        token: String,
        /// Seconds to wait before asking again.
        retry_after_secs: u32,
    },
}

/// Where an adapter can resume a batch that stopped part-way: opaque to
/// the Core, which hands it back through
/// [`Channel::send_batch_resumed`] (from [`Error::PartialSend`] via
/// [`crate::SendOptions::resume_after`]). Its content is the adapter's own
/// (LINE: the parts of the stopped Message already accepted and the retry
/// keys already used); an adapter must ignore a token it cannot read or
/// that does not match the Messages, and then resume as if it had none.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ResumeToken(String);

impl ResumeToken {
    /// Wraps an adapter's encoding.
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }
    /// The adapter's encoding.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
