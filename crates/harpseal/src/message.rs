//! ADR-0003 semantic message model: what a Module builds. Channel adapters
//! render it; a Module never sees channel payloads.

use serde::{Deserialize, Serialize};

/// A language tag such as `th` or `en`. Every Deployment has a default.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Locale(pub String);

impl Locale {
    /// Any language tag.
    pub fn new(tag: impl Into<String>) -> Self {
        Locale(tag.into())
    }
    /// Thai, the default Locale of the first Deployments.
    pub fn th() -> Self {
        Locale("th".into())
    }
    /// English.
    pub fn en() -> Self {
        Locale("en".into())
    }
    /// The tag as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Server-side id of an Action. Invariant: 1..=64 bytes (Telegram's
/// `callback_data` ceiling), checked by its only constructor.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ActionId(String);

impl ActionId {
    /// Longest id in bytes.
    pub const MAX_BYTES: usize = 64;

    /// Rejects an empty id or one over [`ActionId::MAX_BYTES`].
    pub fn new(id: impl Into<String>) -> crate::Result<Self> {
        let id = id.into();
        if id.is_empty() || id.len() > Self::MAX_BYTES {
            return Err(crate::Error::Invalid(format!(
                "Action id must be 1..=64 bytes, got {}",
                id.len()
            )));
        }
        Ok(Self(id))
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for ActionId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A button: a label and the Action it triggers, or a Mini App path.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Button {
    /// Triggers an Action on the server.
    Action {
        /// What the Student sees.
        label: String,
        /// The Action pressed.
        action: ActionId,
    },
    /// Opens a Mini App page; `build()` refuses a Module that declares
    /// these when a Channel has no Mini App.
    MiniApp {
        /// What the Student sees.
        label: String,
        /// Path appended to the Channel's Mini App URL, e.g. `/app`.
        path: String,
    },
}

impl Button {
    /// An Action button.
    pub fn action(label: impl Into<String>, action: ActionId) -> Self {
        Button::Action {
            label: label.into(),
            action,
        }
    }
    /// A Mini App button.
    pub fn mini_app(label: impl Into<String>, path: impl Into<String>) -> Self {
        Button::MiniApp {
            label: label.into(),
            path: path.into(),
        }
    }
    /// The label, whatever the kind.
    pub fn label(&self) -> &str {
        match self {
            Button::Action { label, .. } | Button::MiniApp { label, .. } => label,
        }
    }
    /// The Action id, for Action buttons.
    pub fn action_id(&self) -> Option<&ActionId> {
        match self {
            Button::Action { action, .. } => Some(action),
            Button::MiniApp { .. } => None,
        }
    }
}

/// A structured message with a title, body and optional buttons.
#[derive(Debug, Clone)]
pub struct Card {
    /// Short heading.
    pub title: String,
    /// Plain text body.
    pub body: String,
    /// Buttons under the body.
    pub buttons: Vec<Button>,
}

/// Several Cards shown together; adapters cap the length per channel.
#[derive(Debug, Clone)]
pub struct CardList(pub Vec<Card>);

/// A short set of buttons offered with a text.
#[derive(Debug, Clone)]
pub struct Choices {
    /// The question or prompt.
    pub text: String,
    /// The buttons offered.
    pub options: Vec<Button>,
}

/// A file the Core sends (already in the Deployment's FileStore). Sending
/// one is a Channel Capability, not part of the minimum.
#[derive(Debug, Clone)]
pub struct Attachment {
    /// FileStore key.
    pub file_key: String,
    /// Name shown to the Student.
    pub file_name: String,
    /// MIME type.
    pub content_type: String,
    /// Adapters pick a message type by size.
    pub size_bytes: u64,
    /// An HTTPS URL the platform can fetch (the Kernel mints a signed,
    /// expiring URL from the FileStore before `send`).
    pub url: String,
}

/// One outgoing message, channel-neutral. The Channel minimum must render
/// Text, Card, CardList and Choices.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Message {
    /// Plain text.
    Text(String),
    /// One Card.
    Card(Card),
    /// Several Cards.
    CardList(CardList),
    /// A text with buttons.
    Choices(Choices),
    /// A file.
    Attachment(Attachment),
}

impl Message {
    /// A Text message.
    pub fn text(s: impl Into<String>) -> Self {
        Message::Text(s.into())
    }
    /// Every button the message carries, in order.
    pub fn buttons(&self) -> Vec<&Button> {
        match self {
            Message::Card(c) => c.buttons.iter().collect(),
            Message::CardList(l) => l.0.iter().flat_map(|c| c.buttons.iter()).collect(),
            Message::Choices(c) => c.options.iter().collect(),
            Message::Text(_) | Message::Attachment(_) => Vec::new(),
        }
    }
}

/// One persistent entry point in the chat Menu.
#[derive(Debug, Clone)]
pub struct MenuEntry {
    /// Label in the Student's Locale.
    pub label: String,
    /// The Action the entry triggers.
    pub action: ActionId,
}

/// The Menu set per Channel Account (locked until the Student has a Link).
#[derive(Debug, Clone, Default)]
pub struct Menu {
    /// Entries in display order.
    pub entries: Vec<MenuEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_id_bounds() {
        assert!(ActionId::new("").is_err());
        assert!(ActionId::new("a".repeat(64)).is_ok());
        assert!(ActionId::new("a".repeat(65)).is_err());
    }
}
