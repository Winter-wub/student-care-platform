//! ADR-0003 semantic message model: what a Module builds. Channel adapters
//! render it; a Module never sees channel payloads.

use serde::{Deserialize, Serialize};

mod image;
mod menu_key;
pub use image::CardImage;
pub use menu_key::MenuKey;

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
    /// Opens a web page outside the chat (#108). Build it with
    /// [`Button::url`], which refuses a URL that fails
    /// [`crate::render::link_url`]. The constructor is a convenience, not
    /// the guard (the `fixtures` feature, or a `&mut Button`, can get past
    /// it): every adapter checks again ([`Button::safe_url`]) and shows a
    /// visible line instead of a button it refuses.
    #[non_exhaustive]
    Url {
        /// What the Student sees.
        label: String,
        /// The `https:` URL opened.
        url: String,
        /// Prefer the phone's browser to the in-app one (#113, a PDF):
        /// [`Button::external`]; adapters without the choice ignore it.
        external: bool,
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
    /// A URL button; refuses a URL that is not `https:`, has user info,
    /// unsafe characters or more than [`crate::render::URL_MAX`] bytes
    /// ([`crate::render::link_url`]). The label loses its invisible
    /// characters ([`crate::render::clean_label`]: bidi controls,
    /// zero-width, blank fillers), which could disguise where it leads; a
    /// label left blank becomes the URL's host
    /// ([`crate::render::label_or_host`]), since LINE and Telegram refuse
    /// a message with an empty button label.
    pub fn url(label: impl Into<String>, url: impl Into<String>) -> crate::Result<Self> {
        let url = url.into();
        if !crate::render::link_url(&url) {
            return Err(crate::Error::Invalid(
                "a URL button needs an https: URL".into(),
            ));
        }
        let label: String = label.into();
        Ok(Button::Url {
            label: crate::render::label_or_host(&label, &url),
            url,
            external: false,
        })
    }
    /// This URL button, asking for the phone's own browser (#113).
    #[must_use]
    pub fn external(mut self) -> Self {
        if let Button::Url { external, .. } = &mut self {
            *external = true;
        }
        self
    }
    /// Whether a URL button asks for the phone's own browser.
    pub fn is_external(&self) -> bool {
        matches!(self, Button::Url { external: true, .. })
    }
    /// The label, whatever the kind.
    pub fn label(&self) -> &str {
        match self {
            Button::Action { label, .. }
            | Button::MiniApp { label, .. }
            | Button::Url { label, .. } => label,
        }
    }
    /// The Action id, for Action buttons.
    pub fn action_id(&self) -> Option<&ActionId> {
        match self {
            Button::Action { action, .. } => Some(action),
            Button::MiniApp { .. } | Button::Url { .. } => None,
        }
    }
    /// The URL, for a URL button whose URL passes
    /// [`crate::render::link_url`] (what an adapter renders); `None` for
    /// other kinds and for a URL it must refuse.
    pub fn safe_url(&self) -> Option<&str> {
        match self {
            Button::Url { url, .. } if crate::render::link_url(url) => Some(url),
            _ => None,
        }
    }
}

/// A structured message with a title, body, optional buttons and an
/// optional image above them.
#[derive(Debug, Clone)]
pub struct Card {
    /// Short heading.
    pub title: String,
    /// Plain text body.
    pub body: String,
    /// Buttons under the body.
    pub buttons: Vec<Button>,
    /// An image shown at the top of the Card (#109); `None` for most.
    pub image: Option<CardImage>,
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
    /// A short name of the kind (`text`, `card`, `card_list`, `choices`,
    /// `attachment`), as stored in `outbound_message.kind`.
    pub fn kind(&self) -> &'static str {
        match self {
            Message::Text(_) => "text",
            Message::Card(_) => "card",
            Message::CardList(_) => "card_list",
            Message::Choices(_) => "choices",
            Message::Attachment(_) => "attachment",
        }
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
/// Build it with [`Menu::new`] or [`Menu::keyed`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Menu {
    /// Entries in display order.
    pub entries: Vec<MenuEntry>,
    /// Which Menu this is, independent of its labels (see [`MenuKey`]).
    key: Option<MenuKey>,
    /// Which edit of its key's labels this is (0: none recorded).
    revision: u32,
    /// The Deployment's revision salt, if any.
    salt: Option<String>,
}

impl Menu {
    /// A Menu without a key.
    pub fn new(entries: Vec<MenuEntry>) -> Self {
        Self {
            entries,
            key: None,
            revision: 0,
            salt: None,
        }
    }
    /// A Menu under `key`.
    pub fn keyed(key: MenuKey, entries: Vec<MenuEntry>) -> Self {
        Self::new(entries).with_key(key)
    }
    /// The same Menu under `key`.
    pub fn with_key(mut self, key: MenuKey) -> Self {
        self.key = Some(key);
        self
    }
    /// The Menu key, if any.
    pub fn key(&self) -> Option<&MenuKey> {
        self.key.as_ref()
    }
    /// The same Menu at Menu revision `revision`: the Kernel bumps a
    /// stored revision per key on each back-office save of a Menu label,
    /// so an adapter can name each version apart and retire the previous
    /// one after a re-link ([`crate::Channel::retire_menus`]).
    pub fn with_revision(mut self, revision: u32) -> Self {
        self.revision = revision;
        self
    }
    /// The Menu revision; 0 when none was recorded (no label edit yet,
    /// or a Menu built outside the Kernel).
    pub fn revision(&self) -> u32 {
        self.revision
    }
    /// The same Menu with the Deployment's revision salt: a random tag the
    /// Kernel stores once per Deployment, so that two Deployments on one
    /// platform account never give one revision the same name. An adapter
    /// folds it into the name of a revisioned version and retires only
    /// versions that carry it.
    pub fn with_salt(mut self, salt: impl Into<String>) -> Self {
        self.salt = Some(salt.into());
        self
    }
    /// The revision salt, if any.
    pub fn salt(&self) -> Option<&str> {
        self.salt.as_deref()
    }
}

#[cfg(test)]
mod tests;
