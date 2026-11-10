//! The checks `build()` runs on the declared Module settings (#75):
//! the key, the default, the Permission and the label texts.

use super::SettingSpec;
use crate::error::BuildError;
use crate::manifest::{Catalog, Permission};

/// Adds one Module's declarations, refusing a bad key or default.
pub(crate) fn absorb(
    all: &mut Vec<SettingSpec>,
    new: Vec<SettingSpec>,
) -> core::result::Result<(), BuildError> {
    for s in new {
        if let Some(reason) = invalid(&s, all) {
            return Err(refuse(&s, reason));
        }
        all.push(s);
    }
    Ok(())
}

fn refuse(s: &SettingSpec, reason: String) -> BuildError {
    BuildError::InvalidSetting {
        module: s.module,
        key: s.key.into(),
        reason,
    }
}

/// Checks that need every Module first: a declared Permission, texts in
/// the default Locale.
pub(crate) fn check(
    settings: &[SettingSpec],
    permissions: &[Permission],
    catalog: &Catalog,
) -> core::result::Result<(), BuildError> {
    let locale = catalog.default_locale();
    for s in settings {
        if !permissions.iter().any(|p| p.id == s.permission) {
            return Err(refuse(s, format!("unknown permission `{}`", s.permission)));
        }
        // The Texts page edits catalog keys: a setting key that is also
        // one would put two editors on related names.
        if catalog.knows(s.key) {
            return Err(refuse(
                s,
                "the key is also a text key; give the label its own key".into(),
            ));
        }
        if let Some(k) = [s.label_key, s.help_key]
            .into_iter()
            .find(|k| !catalog.has(&locale, k))
        {
            return Err(refuse(
                s,
                format!("no text for `{k}` in the default Locale"),
            ));
        }
    }
    Ok(())
}

/// Why a declaration is refused, if it is.
fn invalid(spec: &SettingSpec, others: &[SettingSpec]) -> Option<String> {
    let prefix = format!("{}.", spec.module);
    let k = spec.key;
    if !k.starts_with(&prefix) || k.len() == prefix.len() || k.len() > 64 {
        return Some(format!(
            "the key must be `{prefix}<name>`, at most 64 bytes"
        ));
    }
    let chars = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.';
    if !k.bytes().all(chars) {
        return Some("the key may hold only a-z, 0-9, `_` and `.`".into());
    }
    // No Module but the Kernel can start with `kernel.` (`build()` refuses
    // the name first), so the prefix rule above keeps Modules off the
    // Kernel's own rows (`kernel.backup.notified`, …).
    if k == crate::kernel::text::CONSENT_VERSION_KEY {
        return Some("the Consent version is not a Module setting".into());
    }
    if others.iter().any(|o| o.key == k) {
        return Some("declared twice".into());
    }
    let default = spec.default?;
    match spec.kind.check(default) {
        Ok(v) if v == default => None,
        Ok(_) => Some("the default must be stored as is (trimmed, LF line breaks)".into()),
        Err(why) => Some(format!("the default is refused by its kind ({why})")),
    }
}
