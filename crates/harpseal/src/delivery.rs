//! Identity, inbound events, Reply/Push, metering and Channel Capabilities.

use crate::message::ActionId;

/// Milliseconds since the Unix epoch, UTC, from the Deployment's Clock
/// (ADR-0007). Never measure durations with it on Workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub i64);

impl Timestamp {
    /// The value in milliseconds.
    pub fn ms(self) -> i64 {
        self.0
    }
    /// This instant plus `ms` milliseconds.
    pub fn plus_ms(self, ms: i64) -> Self {
        Timestamp(self.0.saturating_add(ms))
    }
}

/// Which Channel, e.g. `ChannelKind("line")`. Unique per Deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelKind(pub &'static str);

impl ChannelKind {
    /// The kind as text, as stored in `channel_account.channel`.
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl core::fmt::Display for ChannelKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0)
    }
}

/// A Student-side account on one Channel.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChannelAccountId {
    /// The Channel.
    pub channel: ChannelKind,
    /// The platform's user id (LINE user id, Telegram chat id).
    pub external_id: String,
}

impl ChannelAccountId {
    /// An account on `channel`.
    pub fn new(channel: ChannelKind, external_id: impl Into<String>) -> Self {
        Self {
            channel,
            external_id: external_id.into(),
        }
    }
}

/// Primary key of a Student row (a UUIDv7, ADR-0007), not the student code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StudentId(pub String);

/// Proof that we may Reply. LINE: a reply token valid for a short window.
/// Channels without the concept never issue one (everything is a Push).
#[derive(Debug, Clone)]
pub struct ReplyToken {
    /// The platform's token.
    pub token: String,
    /// After this instant the token is refused.
    pub expires_at: Timestamp,
}

/// How a message goes out. Chosen by the Core, never by a Module.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Delivery {
    /// In answer to what the Student just sent.
    Reply(ReplyToken),
    /// Started by the system.
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

    /// The reply token, when this is a Reply.
    pub fn reply_token(&self) -> Option<&ReplyToken> {
        match self {
            Delivery::Reply(t) => Some(t),
            Delivery::Push => None,
        }
    }
}

/// Whether one Outbound Message counted against a Quota on its Channel.
/// Reported by the adapter: Reply/Push alone does not decide it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Metering {
    /// Counted against the Channel's Quota.
    Metered,
    /// Not counted.
    Free,
}

/// The platform's id of a sent message (needed to edit or delete it).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlatformMessageId(pub String);

/// What a Channel returns for each send.
#[derive(Debug, Clone)]
pub struct SendReceipt {
    /// The platform's id, when it gives one.
    pub message_id: Option<PlatformMessageId>,
    /// Whether the send was metered.
    pub metering: Metering,
}

/// A raw webhook call, already read from the HTTP layer.
#[derive(Debug, Clone)]
pub struct WebhookRequest {
    /// Request headers (signatures live here).
    pub headers: http::HeaderMap,
    /// Raw body, unparsed: signatures are computed over it.
    pub body: Vec<u8>,
}

/// A file a Student sent, still on the platform until the Core copies it.
#[derive(Debug, Clone)]
pub struct IncomingFile {
    /// The platform's handle for `Channel::download`.
    pub platform_file_id: String,
    /// File name, when the platform gives one.
    pub file_name: Option<String>,
    /// MIME type, when the platform gives one.
    pub content_type: Option<String>,
    /// Size, when the platform gives it.
    pub size_bytes: Option<u64>,
}

/// What a Channel tells the Core happened. The adapter has verified the
/// signature before producing any of these.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Inbound {
    /// The Student added the bot (LINE follow, Telegram `/start`).
    Follow {
        /// Who.
        from: ChannelAccountId,
        /// Reply token, if the Channel issues one.
        reply: Option<ReplyToken>,
    },
    /// Free text.
    Text {
        /// Who.
        from: ChannelAccountId,
        /// What they typed.
        text: String,
        /// Reply token, if the Channel issues one.
        reply: Option<ReplyToken>,
    },
    /// A file.
    File {
        /// Who.
        from: ChannelAccountId,
        /// The file, still on the platform.
        file: IncomingFile,
        /// Reply token, if the Channel issues one.
        reply: Option<ReplyToken>,
    },
    /// A button or Menu entry was pressed.
    Action {
        /// Who.
        from: ChannelAccountId,
        /// Which Action.
        action: ActionId,
        /// Reply token, if the Channel issues one.
        reply: Option<ReplyToken>,
    },
    /// The Student blocked the bot: the Link becomes unreachable. No reply
    /// token: a blocked account cannot be answered.
    Blocked {
        /// Who.
        from: ChannelAccountId,
    },
    /// The Student unblocked the bot.
    Unblocked {
        /// Who.
        from: ChannelAccountId,
        /// Reply token, if the Channel issues one.
        reply: Option<ReplyToken>,
    },
}

impl Inbound {
    /// The account the event came from.
    pub fn from(&self) -> &ChannelAccountId {
        match self {
            Inbound::Follow { from, .. }
            | Inbound::Text { from, .. }
            | Inbound::File { from, .. }
            | Inbound::Action { from, .. }
            | Inbound::Blocked { from }
            | Inbound::Unblocked { from, .. } => from,
        }
    }
}

/// Optional things a Channel can do beyond the minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Capability {
    /// Edit a sent message.
    EditMessage,
    /// Delete a sent message.
    DeleteMessage,
    /// Open a Mini App.
    MiniApp,
    /// Send a file to a Student.
    SendAttachment,
}

/// The set an adapter declares. Derived from its accessors, never configured.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Capabilities {
    /// [`Capability::EditMessage`].
    pub edit_message: bool,
    /// [`Capability::DeleteMessage`].
    pub delete_message: bool,
    /// [`Capability::MiniApp`].
    pub mini_app: bool,
    /// [`Capability::SendAttachment`].
    pub send_attachment: bool,
}

impl Capabilities {
    /// Whether `c` is present.
    pub fn has(&self, c: Capability) -> bool {
        match c {
            Capability::EditMessage => self.edit_message,
            Capability::DeleteMessage => self.delete_message,
            Capability::MiniApp => self.mini_app,
            Capability::SendAttachment => self.send_attachment,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_only_with_an_unexpired_token() {
        let t = ReplyToken {
            token: "r".into(),
            expires_at: Timestamp(100),
        };
        assert!(matches!(
            Delivery::select(Some(&t), Timestamp(99)),
            Delivery::Reply(_)
        ));
        assert!(matches!(
            Delivery::select(Some(&t), Timestamp(100)),
            Delivery::Push
        ));
        assert!(matches!(
            Delivery::select(None, Timestamp(0)),
            Delivery::Push
        ));
    }
}
