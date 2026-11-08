//! Module settings (#75): a typed value per Deployment (not per Locale,
//! unlike Texts), declared by a Module with [`ModuleRegistrar::setting`],
//! edited on `/admin/settings` only by Staff holding the setting's own
//! Permission, and read with [`Settings`] from a page or a Job.
//!
//! Stored in the Kernel's `setting` table with `locale = ''` (the Texts
//! page only edits rows with a Locale), so backups carry them. A missing
//! row means the registered default.

use std::sync::Arc;

use super::ModuleRegistrar;
use super::setting_kind::number;
pub use super::setting_kind::{MAX_LINE_CHARS, MAX_MULTILINE_CHARS, SettingKind};
use crate::error::BuildError;
use crate::manifest::{Catalog, Permission};
use crate::port_types::{Statement, Value};
use crate::{Actor, Error, RequestCtx, Result, Services};

/// One declared setting.
#[derive(Debug, Clone)]
pub(crate) struct SettingSpec {
    pub(crate) module: &'static str,
    pub(crate) key: &'static str,
    pub(crate) kind: SettingKind,
    /// `None`: no default, the setting is unset until its first save.
    pub(crate) default: Option<&'static str>,
    pub(crate) permission: &'static str,
    pub(crate) label_key: &'static str,
    pub(crate) help_key: &'static str,
}

impl ModuleRegistrar {
    /// Declares a typed setting for the whole Deployment, edited on the
    /// back-office page `/admin/settings` only by Staff holding
    /// `permission` (holding `kernel.setting.edit` alone shows it without
    /// a Save button) and read with [`Settings`]. `label_key` and
    /// `help_key` are catalog keys shown on that page; `default` applies
    /// until the first save. Every save writes an audit row
    /// `setting.update` with the value before and after.
    ///
    /// `build()` refuses ([`crate::BuildError::InvalidSetting`]) a key
    /// that does not start with the Module's name and `.`, holds anything
    /// but `a-z`, `0-9`, `_` and `.`, is longer than 64 bytes or declared
    /// twice or is also a catalog text key; a `default` its `kind` refuses; an undeclared
    /// `permission`; and a label or help key without a text in the default
    /// Locale. See `docs/guide/settings.md`.
    pub fn setting(
        &mut self,
        key: &'static str,
        kind: SettingKind,
        default: &'static str,
        permission: &'static str,
        label_key: &'static str,
        help_key: &'static str,
    ) -> &mut Self {
        self.declare(key, kind, Some(default), permission, label_key, help_key)
    }

    /// Like [`ModuleRegistrar::setting`] but with no default (#103): for a
    /// value no placeholder can stand in for, such as the bank account
    /// Students pay into. Reading it returns [`Error::SettingNotSet`]
    /// until the first save, and `/admin/settings` shows it as "not set
    /// yet". Everything else (the checks at `build()`, the Permission,
    /// the audit row) is as for `setting`; the first save's audit row has
    /// a `null` value before.
    pub fn setting_without_default(
        &mut self,
        key: &'static str,
        kind: SettingKind,
        permission: &'static str,
        label_key: &'static str,
        help_key: &'static str,
    ) -> &mut Self {
        self.declare(key, kind, None, permission, label_key, help_key)
    }

    fn declare(
        &mut self,
        key: &'static str,
        kind: SettingKind,
        default: Option<&'static str>,
        permission: &'static str,
        label_key: &'static str,
        help_key: &'static str,
    ) -> &mut Self {
        self.settings.push(SettingSpec {
            module: self.module,
            key,
            kind,
            default,
            permission,
            label_key,
            help_key,
        });
        self
    }
}

/// A setting's value, typed by its [`SettingKind`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingValue {
    /// [`SettingKind::Text`], [`SettingKind::MultilineText`] or
    /// [`SettingKind::AccountNumber`].
    Text(String),
    /// [`SettingKind::Number`].
    Number(i64),
    /// [`SettingKind::Date`], as `YYYY-MM-DD`.
    Date(String),
    /// [`SettingKind::Url`].
    Url(String),
}

impl SettingValue {
    fn of(kind: SettingKind, v: String) -> Self {
        match kind {
            SettingKind::Number => SettingValue::Number(number(&v).unwrap_or_default()),
            SettingKind::Date => SettingValue::Date(v),
            SettingKind::Url => SettingValue::Url(v),
            SettingKind::Text | SettingKind::MultilineText | SettingKind::AccountNumber => {
                SettingValue::Text(v)
            }
        }
    }

    /// The value as text (a number in decimal).
    pub fn as_text(&self) -> String {
        match self {
            SettingValue::Text(t) | SettingValue::Date(t) | SettingValue::Url(t) => t.clone(),
            SettingValue::Number(n) => n.to_string(),
        }
    }
}

/// The stored row of a setting: (value, updated_at, updated_by).
pub(crate) type Stored = (String, i64, Option<String>);

/// Reads the Module settings of a Deployment: always the latest saved
/// value (no cache), or the default. Get one from
/// [`crate::PageState::settings`], [`crate::JobContext::settings`] or
/// [`crate::Deployment::settings`].
#[derive(Clone)]
pub struct Settings {
    services: Services,
    specs: Arc<[SettingSpec]>,
}

impl Settings {
    pub(crate) fn new(services: Services, specs: Arc<[SettingSpec]>) -> Self {
        Self { services, specs }
    }

    pub(crate) fn specs(&self) -> &[SettingSpec] {
        &self.specs
    }

    pub(crate) fn spec(&self, key: &str) -> Option<&SettingSpec> {
        self.specs.iter().find(|s| s.key == key)
    }

    /// The stored row of `key`, if any.
    pub(crate) async fn stored(&self, key: &str) -> Result<Option<Stored>> {
        let rows = self
            .services
            .db
            .query(
                Statement::new(
                    "SELECT value, updated_at, updated_by FROM setting \
                     WHERE key = ? AND locale = ''",
                )
                .bind(key),
            )
            .await?;
        let Some(value) = rows.get(0, "value").and_then(Value::as_text) else {
            return Ok(None);
        };
        let at = rows.get(0, "updated_at").and_then(Value::as_i64);
        let by = rows.get(0, "updated_by").and_then(Value::as_text);
        Ok(Some((
            value.to_string(),
            at.unwrap_or_default(),
            by.map(str::to_string),
        )))
    }

    /// The value of `key`: the saved one, or the default when none is
    /// saved ([`Error::SettingNotSet`] for a setting declared without a
    /// default, until its first save). An undeclared key is an [`Error::Invalid`] (a programming
    /// error). A saved value that no longer passes its kind's check (a
    /// restored row, a Module that changed the kind or a check tightened
    /// since) is [`Error::SettingNotSet`], never the default in its place,
    /// which could show Students an outdated value such as the built-in
    /// bank account: answer that one with "not set" (`/admin/settings`
    /// flags the value for Staff to save again) and any other error as a
    /// failure.
    pub async fn get(&self, key: &str) -> Result<SettingValue> {
        let spec = self
            .spec(key)
            .ok_or_else(|| Error::Invalid(format!("no Module declares setting `{key}`")))?;
        let value = match self.stored(key).await? {
            None => match spec.default {
                Some(d) => d.to_string(),
                None => return Err(Error::SettingNotSet(key.to_string())),
            },
            Some((v, _, _)) => match spec.kind.check(&v) {
                Ok(ok) if ok == v => v,
                _ => return Err(Error::SettingNotSet(key.to_string())),
            },
        };
        Ok(SettingValue::of(spec.kind, value))
    }

    /// Saves `value` as the value of `key`: for a Deployment's seed, CLI
    /// or Job code that must set a Setting (#103), through the very path
    /// of `/admin/settings`. The kind's check applies and the value is
    /// stored as the check returns it; a compare-and-set write and its
    /// `setting.update` audit row (value before, `null` while unset, and
    /// after) go in one batch. Saving the value a setting already has
    /// writes nothing.
    ///
    /// `actor` decides the Permission check: Staff must be active and hold
    /// the setting's own Permission (a CLI cannot bypass Roles, as with
    /// `roster::upload`), [`Actor::System`] and [`Actor::Job`] are the
    /// Deployment's own code and need none, and a Student is
    /// [`Error::Forbidden`]. An undeclared key or a value the kind refuses
    /// is [`Error::Invalid`]; a row changed by someone else meanwhile is
    /// [`Error::Conflict`] (nothing written).
    pub async fn save(
        &self,
        key: &str,
        value: &str,
        actor: &Actor,
        ctx: &RequestCtx,
    ) -> Result<()> {
        let spec = self
            .spec(key)
            .ok_or_else(|| Error::Invalid(format!("no Module declares setting `{key}`")))?;
        match actor {
            Actor::Staff(_) => {
                crate::auth::roles::require(&self.services, actor, spec.permission).await?;
            }
            Actor::System | Actor::Job(_) => {}
            _ => {
                return Err(Error::Forbidden {
                    permission: spec.permission.to_string(),
                });
            }
        }
        let value = spec
            .kind
            .check(value)
            .map_err(|why| Error::Invalid(format!("setting `{key}`: {why}")))?;
        let stored = self.stored(key).await?.map(|(v, _, _)| v);
        let before = stored.as_deref().or(spec.default);
        if before == Some(value.as_str()) {
            return Ok(());
        }
        let saved = crate::kernel::admin::write_setting(
            &self.services,
            actor,
            spec,
            &value,
            stored.as_deref(),
            ctx,
        );
        if saved.await? {
            Ok(())
        } else {
            Err(Error::Conflict(format!(
                "setting `{key}` changed meanwhile"
            )))
        }
    }

    /// The value of `key` as text, whatever its kind.
    pub async fn text(&self, key: &str) -> Result<String> {
        Ok(self.get(key).await?.as_text())
    }

    /// The value of a [`SettingKind::Number`] setting; another kind is an
    /// [`Error::Invalid`].
    pub async fn number(&self, key: &str) -> Result<i64> {
        match self.get(key).await? {
            SettingValue::Number(n) => Ok(n),
            _ => Err(Error::Invalid(format!("setting `{key}` is not a number"))),
        }
    }
}

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
