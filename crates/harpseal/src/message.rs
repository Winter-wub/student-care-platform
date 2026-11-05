//! ADR-0003 semantic message model: what a Module builds. Channel adapters
//! render it; a Module never sees channel payloads.

use serde::{Deserialize, Serialize};

/// A language tag such as `th` or `en`. Every Deployment has a default.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Locale(pub String);

impl Locale {
    pub fn th() -> Self {
        Locale("th".into())
    }
}

/// Server-side id of an Action. Invariant: 1..=64 bytes (Telegram's
/// `callback_data` ceiling), checked on construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionId(String);

impl ActionId {
    pub const MAX_BYTES: usize = 64;

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

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A button: a label and the Action it triggers, or a Mini App path.
#[derive(Debug, Clone)]
pub enum Button {
    Action {
        label: String,
        action: ActionId,
    },
    /// Opens a Mini App page; adapters without the capability degrade it.
    MiniApp {
        label: String,
        path: String,
    },
}

#[derive(Debug, Clone)]
pub struct Card {
    pub title: String,
    pub body: String,
    pub buttons: Vec<Button>,
}

/// Several Cards shown together; adapters cap the length per channel.
#[derive(Debug, Clone)]
pub struct CardList(pub Vec<Card>);

/// A short set of buttons offered with a text.
#[derive(Debug, Clone)]
pub struct Choices {
    pub text: String,
    pub options: Vec<Button>,
}

/// A file the Core sends (already in the Deployment's FileStore).
#[derive(Debug, Clone)]
pub struct Attachment {
    pub file_key: String,
    pub file_name: String,
    pub content_type: String,
    /// Added for issue #3: adapters pick a message type by size.
    pub size_bytes: u64,
    /// Added for issue #3: an HTTPS URL the platform can fetch (the Kernel
    /// mints a signed, expiring URL from the FileStore before `send`).
    pub url: String,
}

/// One outgoing message, channel-neutral. The Channel minimum must render
/// Text, Card, CardList and Choices.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Message {
    Text(String),
    Card(Card),
    CardList(CardList),
    Choices(Choices),
    Attachment(Attachment),
}

impl Message {
    pub fn text(s: impl Into<String>) -> Self {
        Message::Text(s.into())
    }
}

/// One persistent entry point in the chat Menu.
#[derive(Debug, Clone)]
pub struct MenuEntry {
    pub label: String,
    pub action: ActionId,
}

/// The Menu set per Channel Account (locked until the Student has a Link).
#[derive(Debug, Clone, Default)]
pub struct Menu {
    pub entries: Vec<MenuEntry>,
}
