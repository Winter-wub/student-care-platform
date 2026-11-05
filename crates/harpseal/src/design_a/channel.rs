//! Design A Channel: one object-safe trait; optional Capabilities are
//! accessor methods returning `Option<&dyn _>`.

use async_trait::async_trait;

use crate::Result;
use crate::delivery::{
    Capabilities, ChannelAccountId, ChannelKind, Delivery, Inbound, IncomingFile,
    PlatformMessageId, SendReceipt, WebhookRequest,
};
use crate::message::{Menu, Message};

/// A messaging platform adapter. The required minimum:
/// receive text and files (`parse_webhook` + `download`), send
/// Text/Card/CardList/Choices (`send`), set the Menu per Channel Account
/// (`set_menu`) and report metered/free per message (`SendReceipt`).
#[async_trait]
pub trait Channel: Send + Sync {
    /// Unique per Deployment. A method, not a const: consts break `dyn`.
    fn kind(&self) -> ChannelKind;

    /// Verify the signature and turn one webhook call into events.
    /// `Err(BadSignature)` must be returned before any parsing.
    async fn parse_webhook(&self, req: WebhookRequest) -> Result<Vec<Inbound>>;

    /// Fetch the bytes of a file a Student sent so the Core can copy it.
    async fn download(&self, file: &IncomingFile) -> Result<Vec<u8>>;

    /// Render and send one message. The adapter degrades what it cannot show
    /// and reports whether the platform metered it.
    async fn send(
        &self,
        to: &ChannelAccountId,
        delivery: &Delivery,
        msg: &Message,
    ) -> Result<SendReceipt>;

    /// Set (or with `None`, reset to the locked pre-Link) Menu for one account.
    async fn set_menu(&self, to: &ChannelAccountId, menu: Option<&Menu>) -> Result<()>;

    /// Channel Capability: edit a sent message.
    fn editor(&self) -> Option<&dyn EditMessage> {
        None
    }
    /// Channel Capability: delete a sent message.
    fn deleter(&self) -> Option<&dyn DeleteMessage> {
        None
    }
    /// Channel Capability: Mini App (LIFF / Telegram Web App).
    fn mini_app(&self) -> Option<&dyn MiniApp> {
        None
    }

    /// Derived from the accessors above so the two cannot disagree.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            edit_message: self.editor().is_some(),
            delete_message: self.deleter().is_some(),
            mini_app: self.mini_app().is_some(),
        }
    }
}

#[async_trait]
pub trait EditMessage: Send + Sync {
    async fn edit(&self, to: &ChannelAccountId, id: &PlatformMessageId, msg: &Message)
    -> Result<()>;
}

#[async_trait]
pub trait DeleteMessage: Send + Sync {
    async fn delete(&self, to: &ChannelAccountId, id: &PlatformMessageId) -> Result<()>;
}

#[async_trait]
pub trait MiniApp: Send + Sync {
    /// URL that opens `path` inside the chat app.
    fn launch_url(&self, path: &str) -> String;
    /// Resolve the platform's signed token to a Channel Account (App Session).
    async fn verify_session(&self, signed_token: &str) -> Result<ChannelAccountId>;
}
