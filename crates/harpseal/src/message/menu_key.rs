//! [`MenuKey`]: which Menu a [`super::Menu`] is (moved out of
//! `message.rs`, which was at 400 lines).

use super::Locale;

/// Which Menu a [`super::Menu`] is, stable while its labels change (a `setting`
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
