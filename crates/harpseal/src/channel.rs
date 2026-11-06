//! Channel: one object-safe trait; optional Capabilities are accessor
//! methods returning `Option<&dyn _>`, so the declaration and the behaviour
//! cannot disagree.

use async_trait::async_trait;

use crate::Result;
use crate::delivery::{
    Capabilities, ChannelAccountId, ChannelKind, Delivery, Inbound, IncomingFile,
    PlatformMessageId, SendReceipt, WebhookRequest,
};
use crate::message::{Attachment, Locale, Menu, Message};

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

    /// Sets the Menu for one account; `None` resets it to the locked
    /// pre-Link Menu.
    async fn set_menu(&self, to: &ChannelAccountId, menu: Option<&Menu>) -> Result<()>;

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

    /// Derived from the accessors above.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            edit_message: self.editor().is_some(),
            delete_message: self.deleter().is_some(),
            mini_app: self.mini_app().is_some(),
            send_attachment: self.attachments().is_some(),
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
