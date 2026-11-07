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
}

impl Menu {
    /// A Menu without a key.
    pub fn new(entries: Vec<MenuEntry>) -> Self {
        Self { entries, key: None }
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
}

/// Which Menu a [`Menu`] is, stable while its labels change (a `setting`
/// override, say), so that an adapter can tell its versions apart from
/// other Menus: the Locale an unlocked Menu serves
/// ([`MenuKey::for_locale`]), or [`MenuKey::LOCKED`] for the locked Menu.
/// Invariant: 1 to [`MenuKey::MAX_LEN`] of `a-z0-9`, checked by
/// [`MenuKey::new`]; [`MenuKey::LOCKED`] only from [`MenuKey::locked`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MenuKey(String);

impl MenuKey {
    /// Longest key.
    pub const MAX_LEN: usize = 16;
    /// The locked Menu's key, reserved for the adapters' locked Menu: no
    /// Locale maps to it and [`MenuKey::new`] refuses it.
    pub const LOCKED: &'static str = "locked";

    /// Rejects anything but 1 to [`MenuKey::MAX_LEN`] lowercase ASCII
    /// letters and digits, and the reserved [`MenuKey::LOCKED`] (an
    /// unlocked Menu under it would be taken for the locked one).
    pub fn new(key: impl Into<String>) -> crate::Result<Self> {
        let key = key.into();
        let ok = !key.is_empty()
            && key.len() <= Self::MAX_LEN
            && key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        if key == Self::LOCKED {
            Err(crate::Error::Invalid(format!(
                "Menu key {key:?} is reserved for the locked Menu"
            )))
        } else if ok {
            Ok(Self(key))
        } else {
            Err(crate::Error::Invalid(format!(
                "Menu key {key:?}: 1 to {} of a-z and 0-9",
                Self::MAX_LEN
            )))
        }
    }

    /// The locked Menu's key, [`MenuKey::LOCKED`].
    pub fn locked() -> Self {
        Self(Self::LOCKED.into())
    }

    /// The key of the unlocked Menu in `locale`: the tag in lowercase
    /// without `-` and `_` (`zh-Hant` -> `zhhant`). The one mapping every
    /// adapter relies on. Fails when the result is not a valid key (too
    /// long, other characters) or is [`MenuKey::LOCKED`]. The Kernel keys
    /// by the *effective* Locale ([`crate::Catalog::effective`]), and
    /// `build()` checks every catalog Locale maps to its own key.
    pub fn for_locale(locale: &Locale) -> crate::Result<Self> {
        let mapped: String = locale
            .as_str()
            .chars()
            .filter(|c| !matches!(c, '-' | '_'))
            .flat_map(char::to_lowercase)
            .collect();
        Self::new(mapped)
    }

    /// The key as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
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

    #[test]
    fn menu_keys_from_locales() {
        let key = |tag: &str| MenuKey::for_locale(&Locale::new(tag)).map(|k| k.0);
        assert_eq!(key("th").unwrap(), "th");
        assert_eq!(key("zh-hant").unwrap(), "zhhant");
        assert_eq!(key("zh-Hant").unwrap(), "zhhant");
        assert_eq!(key("pt_BR").unwrap(), "ptbr");
        assert!(key("").is_err());
        assert!(key("locked").is_err());
        assert!(key("x-a-very-long-private-tag").is_err());
        assert!(key("th.u").is_err());
    }

    #[test]
    fn menu_key_bounds() {
        assert!(MenuKey::new("th").is_ok());
        assert!(MenuKey::new("a".repeat(16)).is_ok());
        assert!(MenuKey::new("a".repeat(17)).is_err());
        assert!(MenuKey::new("").is_err());
        assert!(MenuKey::new("TH").is_err());
        assert!(MenuKey::new("zh-hant").is_err());
        assert!(MenuKey::new("locked").is_err(), "reserved");
        assert_eq!(MenuKey::locked().as_str(), MenuKey::LOCKED);
        let m = Menu::new(Vec::new());
        assert_eq!(m.key(), None);
        let m = m.with_key(MenuKey::locked());
        assert_eq!(m.key().map(MenuKey::as_str), Some("locked"));
    }
}
