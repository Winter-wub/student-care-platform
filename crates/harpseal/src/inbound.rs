//! What a Channel hands the Core from a webhook: the raw call, and the
//! events parsed from it. Re-exported through `crate::delivery`.

use crate::delivery::{ChannelAccountId, ReplyToken};
use crate::message::ActionId;

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
///
/// Every variant carries the platform's event id (`event_id`: LINE
/// `webhookEventId`, Telegram `update_id`; `None` when the platform has
/// none) and whether the platform says this is a redelivery (`redelivery`:
/// LINE `deliveryContext.isRedelivery`; `false` elsewhere). The Kernel
/// handles each event id at most once per Channel, so a platform retry
/// never answers twice.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Inbound {
    /// The Student added the bot (LINE follow, Telegram `/start`).
    Follow {
        /// Who.
        from: ChannelAccountId,
        /// Reply token, if the Channel issues one.
        reply: Option<ReplyToken>,
        /// The platform's event id, if it has one.
        event_id: Option<String>,
        /// The platform marked this event as a redelivery.
        redelivery: bool,
    },
    /// Free text.
    Text {
        /// Who.
        from: ChannelAccountId,
        /// What they typed.
        text: String,
        /// Reply token, if the Channel issues one.
        reply: Option<ReplyToken>,
        /// The platform's event id, if it has one.
        event_id: Option<String>,
        /// The platform marked this event as a redelivery.
        redelivery: bool,
    },
    /// A file.
    File {
        /// Who.
        from: ChannelAccountId,
        /// The file, still on the platform.
        file: IncomingFile,
        /// Reply token, if the Channel issues one.
        reply: Option<ReplyToken>,
        /// The platform's event id, if it has one.
        event_id: Option<String>,
        /// The platform marked this event as a redelivery.
        redelivery: bool,
    },
    /// A button or Menu entry was pressed.
    Action {
        /// Who.
        from: ChannelAccountId,
        /// Which Action.
        action: ActionId,
        /// Reply token, if the Channel issues one.
        reply: Option<ReplyToken>,
        /// The platform's event id, if it has one.
        event_id: Option<String>,
        /// The platform marked this event as a redelivery.
        redelivery: bool,
    },
    /// The Student blocked the bot: the Link becomes unreachable. No reply
    /// token: a blocked account cannot be answered.
    Blocked {
        /// Who.
        from: ChannelAccountId,
        /// The platform's event id, if it has one.
        event_id: Option<String>,
        /// The platform marked this event as a redelivery.
        redelivery: bool,
    },
    /// The Student unblocked the bot.
    Unblocked {
        /// Who.
        from: ChannelAccountId,
        /// Reply token, if the Channel issues one.
        reply: Option<ReplyToken>,
        /// The platform's event id, if it has one.
        event_id: Option<String>,
        /// The platform marked this event as a redelivery.
        redelivery: bool,
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
            | Inbound::Blocked { from, .. }
            | Inbound::Unblocked { from, .. } => from,
        }
    }

    /// The reply token, when the event carries one.
    pub fn reply(&self) -> Option<&ReplyToken> {
        match self {
            Inbound::Follow { reply, .. }
            | Inbound::Text { reply, .. }
            | Inbound::File { reply, .. }
            | Inbound::Action { reply, .. }
            | Inbound::Unblocked { reply, .. } => reply.as_ref(),
            Inbound::Blocked { .. } => None,
        }
    }

    /// The platform's event id, when it has one.
    pub fn event_id(&self) -> Option<&str> {
        match self {
            Inbound::Follow { event_id, .. }
            | Inbound::Text { event_id, .. }
            | Inbound::File { event_id, .. }
            | Inbound::Action { event_id, .. }
            | Inbound::Blocked { event_id, .. }
            | Inbound::Unblocked { event_id, .. } => event_id.as_deref(),
        }
    }

    /// Whether the platform marked the event as a redelivery.
    pub fn redelivery(&self) -> bool {
        match self {
            Inbound::Follow { redelivery, .. }
            | Inbound::Text { redelivery, .. }
            | Inbound::File { redelivery, .. }
            | Inbound::Action { redelivery, .. }
            | Inbound::Blocked { redelivery, .. }
            | Inbound::Unblocked { redelivery, .. } => *redelivery,
        }
    }

    /// The same event with `event_id` and `redelivery` set, whatever its
    /// variant (adapters build the event, then stamp the platform's id).
    pub fn with_event_id(mut self, id: Option<String>, is_redelivery: bool) -> Self {
        match &mut self {
            Inbound::Follow {
                event_id,
                redelivery,
                ..
            }
            | Inbound::Text {
                event_id,
                redelivery,
                ..
            }
            | Inbound::File {
                event_id,
                redelivery,
                ..
            }
            | Inbound::Action {
                event_id,
                redelivery,
                ..
            }
            | Inbound::Blocked {
                event_id,
                redelivery,
                ..
            }
            | Inbound::Unblocked {
                event_id,
                redelivery,
                ..
            } => {
                *event_id = id;
                *redelivery = is_redelivery;
            }
        }
        self
    }

    /// A short name of the event kind (`follow`, `text`, `file`, `action`,
    /// `blocked`, `unblocked`), for logs.
    pub fn kind(&self) -> &'static str {
        match self {
            Inbound::Follow { .. } => "follow",
            Inbound::Text { .. } => "text",
            Inbound::File { .. } => "file",
            Inbound::Action { .. } => "action",
            Inbound::Blocked { .. } => "blocked",
            Inbound::Unblocked { .. } => "unblocked",
        }
    }
}
