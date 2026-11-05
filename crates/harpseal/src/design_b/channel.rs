//! Design B Channel: associated consts and types. Optional Capabilities are
//! associated types; an adapter without one names [`Unsupported`].

use std::future::Future;

use crate::delivery::{
    Capabilities, Capability, ChannelAccountId, ChannelKind, Delivery, Inbound, IncomingFile,
    PlatformMessageId, SendReceipt, WebhookRequest,
};
use crate::message::{Menu, Message};
use crate::{Error, Result};

/// Same required minimum as Design A, statically dispatched.
pub trait Channel: Send + Sync + 'static {
    const KIND: ChannelKind;
    /// Stable Rust has no associated-type defaults: every adapter writes all
    /// three, using `Unsupported` for what it lacks.
    type Editor: EditMessage;
    type Deleter: DeleteMessage;
    type MiniApp: MiniApp;

    fn parse_webhook(&self, req: WebhookRequest) -> impl Future<Output = Result<Vec<Inbound>>> + Send;
    fn download(&self, file: &IncomingFile) -> impl Future<Output = Result<Vec<u8>>> + Send;
    fn send(
        &self,
        to: &ChannelAccountId,
        delivery: &Delivery,
        msg: &Message,
    ) -> impl Future<Output = Result<SendReceipt>> + Send;
    fn set_menu(&self, to: &ChannelAccountId, menu: Option<&Menu>) -> impl Future<Output = Result<()>> + Send;

    fn editor(&self) -> Option<&Self::Editor> {
        None
    }
    fn deleter(&self) -> Option<&Self::Deleter> {
        None
    }
    fn mini_app(&self) -> Option<&Self::MiniApp> {
        None
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            edit_message: self.editor().is_some(),
            delete_message: self.deleter().is_some(),
            mini_app: self.mini_app().is_some(),
        }
    }
}

pub trait EditMessage: Send + Sync + 'static {
    fn edit(&self, to: &ChannelAccountId, id: &PlatformMessageId, msg: &Message)
    -> impl Future<Output = Result<()>> + Send;
}

pub trait DeleteMessage: Send + Sync + 'static {
    fn delete(&self, to: &ChannelAccountId, id: &PlatformMessageId) -> impl Future<Output = Result<()>> + Send;
}

pub trait MiniApp: Send + Sync + 'static {
    fn launch_url(&self, path: &str) -> String;
    fn verify_session(&self, signed_token: &str) -> impl Future<Output = Result<ChannelAccountId>> + Send;
}

/// Uninhabited: `Option<&Unsupported>` is always `None`, and the compiler
/// can drop the unreachable branch.
pub enum Unsupported {}

impl EditMessage for Unsupported {
    async fn edit(&self, _: &ChannelAccountId, _: &PlatformMessageId, _: &Message) -> Result<()> {
        match *self {}
    }
}
impl DeleteMessage for Unsupported {
    async fn delete(&self, _: &ChannelAccountId, _: &PlatformMessageId) -> Result<()> {
        match *self {}
    }
}
impl MiniApp for Unsupported {
    fn launch_url(&self, _: &str) -> String {
        match *self {}
    }
    async fn verify_session(&self, _: &str) -> Result<ChannelAccountId> {
        match *self {}
    }
}

/// A type-level list of Channels: `()` or `(Head, Tail)`. Dispatch walks the
/// list comparing `KIND`; `Ok(None)` means no Channel of that kind.
pub trait ChannelSet: Send + Sync + 'static {
    fn kinds(&self, out: &mut Vec<ChannelKind>);
    fn parse_webhook(&self, kind: &str, req: WebhookRequest)
    -> impl Future<Output = Result<Option<Vec<Inbound>>>> + Send;
    fn send(&self, to: &ChannelAccountId, d: &Delivery, m: &Message)
    -> impl Future<Output = Result<Option<SendReceipt>>> + Send;
    fn edit(&self, to: &ChannelAccountId, id: &PlatformMessageId, m: &Message)
    -> impl Future<Output = Result<Option<()>>> + Send;
}

impl ChannelSet for () {
    fn kinds(&self, _: &mut Vec<ChannelKind>) {}
    async fn parse_webhook(&self, _: &str, _: WebhookRequest) -> Result<Option<Vec<Inbound>>> {
        Ok(None)
    }
    async fn send(&self, _: &ChannelAccountId, _: &Delivery, _: &Message) -> Result<Option<SendReceipt>> {
        Ok(None)
    }
    async fn edit(&self, _: &ChannelAccountId, _: &PlatformMessageId, _: &Message) -> Result<Option<()>> {
        Ok(None)
    }
}

impl<H: Channel, T: ChannelSet> ChannelSet for (H, T) {
    fn kinds(&self, out: &mut Vec<ChannelKind>) {
        out.push(H::KIND);
        self.1.kinds(out);
    }
    async fn parse_webhook(&self, kind: &str, req: WebhookRequest) -> Result<Option<Vec<Inbound>>> {
        if kind == H::KIND.0 {
            self.0.parse_webhook(req).await.map(Some)
        } else {
            self.1.parse_webhook(kind, req).await
        }
    }
    async fn send(&self, to: &ChannelAccountId, d: &Delivery, m: &Message) -> Result<Option<SendReceipt>> {
        if to.channel == H::KIND {
            self.0.send(to, d, m).await.map(Some)
        } else {
            self.1.send(to, d, m).await
        }
    }
    async fn edit(&self, to: &ChannelAccountId, id: &PlatformMessageId, m: &Message) -> Result<Option<()>> {
        if to.channel != H::KIND {
            return self.1.edit(to, id, m).await;
        }
        match self.0.editor() {
            Some(e) => e.edit(to, id, m).await.map(Some),
            None => Err(Error::Unsupported(Capability::EditMessage)),
        }
    }
}
