//! Identity, inbound events, Reply/Push, metering and Channel Capabilities.

use crate::message::ActionId;

/// Milliseconds since the Unix epoch, from the Deployment's Clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(pub i64);

/// Which Channel, e.g. `ChannelKind("line")`. Unique per Deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelKind(pub &'static str);

/// A Student-side account on one Channel.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChannelAccountId {
    pub channel: ChannelKind,
    pub external_id: String,
}

/// Roster key of a Student.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StudentId(pub String);

/// Proof that we may Reply. LINE: a reply token valid for a short window.
/// Channels without the concept never issue one (everything is a Push).
#[derive(Debug, Clone)]
pub struct ReplyToken {
    pub token: String,
    pub expires_at: Timestamp,
}

/// How a message goes out. Chosen by the Core, never by a Module.
#[derive(Debug, Clone)]
pub enum Delivery {
    Reply(ReplyToken),
    Push,
}

impl Delivery {
    /// Reply when an unexpired token exists, else Push. The only place the
    /// Reply/Push choice is made.
    pub fn select(token: Option<&ReplyToken>, now: Timestamp) -> Delivery {
        match token {
            Some(t) if t.expires_at > now => Delivery::Reply(t.clone()),
            _ => Delivery::Push,
        }
    }
}

/// Whether one Outbound Message counted against a Quota on its Channel.
/// Reported by the adapter: Reply/Push alone does not decide it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metering {
    Metered,
    Free,
}

/// The platform's id of a sent message (needed to edit or delete it).
#[derive(Debug, Clone)]
pub struct PlatformMessageId(pub String);

/// What a Channel returns for each send.
#[derive(Debug, Clone)]
pub struct SendReceipt {
    pub message_id: Option<PlatformMessageId>,
    pub metering: Metering,
}

/// The stored record of every send; usage figures come from these.
#[derive(Debug, Clone)]
pub struct OutboundMessage {
    pub to: ChannelAccountId,
    pub push: bool,
    pub metering: Metering,
    pub message_id: Option<PlatformMessageId>,
    pub sent_at: Timestamp,
}

/// A raw webhook call, already read from the HTTP layer.
#[derive(Debug, Clone)]
pub struct WebhookRequest {
    pub headers: http::HeaderMap,
    pub body: Vec<u8>,
}

/// A file a Student sent, still on the platform until the Core copies it.
#[derive(Debug, Clone)]
pub struct IncomingFile {
    pub platform_file_id: String,
    pub file_name: Option<String>,
    pub content_type: Option<String>,
    pub size_bytes: Option<u64>,
}

/// What a Channel tells the Core happened. Adapter verified the signature.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Inbound {
    Text {
        from: ChannelAccountId,
        text: String,
        reply: Option<ReplyToken>,
    },
    File {
        from: ChannelAccountId,
        file: IncomingFile,
        reply: Option<ReplyToken>,
    },
    Action {
        from: ChannelAccountId,
        action: ActionId,
        reply: Option<ReplyToken>,
    },
    /// The Student blocked the bot: Link becomes unreachable.
    Blocked { from: ChannelAccountId },
}

/// Optional things a Channel can do beyond the minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    EditMessage,
    DeleteMessage,
    MiniApp,
}

/// The set an adapter declares. Derived from the adapter, never configured.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub edit_message: bool,
    pub delete_message: bool,
    pub mini_app: bool,
}

impl Capabilities {
    pub fn has(&self, c: Capability) -> bool {
        match c {
            Capability::EditMessage => self.edit_message,
            Capability::DeleteMessage => self.delete_message,
            Capability::MiniApp => self.mini_app,
        }
    }
}
