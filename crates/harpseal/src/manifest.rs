//! What a Module contributes, as data.

use std::collections::HashMap;

use crate::message::{ActionId, Locale};

/// One Permission a Module declares. Ids are `module.thing.verb`, global
/// across the Deployment; Roles group them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permission {
    /// Global id, e.g. `kernel.student.view`.
    pub id: &'static str,
    /// What it allows, for the Roles page.
    pub description: &'static str,
}

impl Permission {
    /// A Permission, usable in `const` items.
    pub const fn new(id: &'static str, description: &'static str) -> Self {
        Self { id, description }
    }
}

/// A Menu entry: the label is a catalog key, the Action an [`ActionId`].
#[derive(Debug, Clone)]
pub struct MenuEntrySpec {
    /// Catalog key of the label; `build()` checks it exists.
    pub label_key: &'static str,
    /// The Action the entry triggers.
    pub action: ActionId,
    /// Order in the Menu; lower first.
    pub order: u16,
}

/// A default text in one Locale. `[texts]` in the TOML, then a `setting`
/// row, override it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogEntry {
    /// Key, `<module>.<area>.<name>`.
    pub key: &'static str,
    /// Locale tag.
    pub locale: &'static str,
    /// The text.
    pub text: &'static str,
}

impl CatalogEntry {
    /// An entry, usable in `const` items.
    pub const fn new(key: &'static str, locale: &'static str, text: &'static str) -> Self {
        Self { key, locale, text }
    }
}

/// A kind of Job. `schedule` is a cron expression for recurring Jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobSpec {
    /// `module.name`, stored in `job.kind`.
    pub kind: &'static str,
    /// Cron expression for a recurring Job, evaluated in the Deployment's
    /// `utc_offset`. Five fields: minute (0-59), hour (0-23), day of month
    /// (1-31), month (1-12), day of week (0-7, 0 and 7 = Sunday); each a
    /// comma-separated list of `*`, `*/n`, `a` or `a-b`. Nothing else
    /// (names, `a-b/n`, `@daily`, `L`, `?`) is accepted: `build()` refuses
    /// it with [`crate::BuildError::InvalidSchedule`]. When both day fields
    /// are restricted (neither starts with `*`), either one matching is
    /// enough (Vixie cron); otherwise both must match. Each
    /// due minute enqueues one Job with payload `{"scheduled_at": ms}`,
    /// once, however many ticks run in that minute; a minute with no tick
    /// at all (and none in the minute after) is skipped, not caught up.
    pub schedule: Option<&'static str>,
}

impl JobSpec {
    /// A Job kind enqueued by code only.
    pub const fn new(kind: &'static str) -> Self {
        Self {
            kind,
            schedule: None,
        }
    }
    /// A Job kind also enqueued by the scheduler on `cron`.
    pub const fn scheduled(kind: &'static str, cron: &'static str) -> Self {
        Self {
            kind,
            schedule: Some(cron),
        }
    }
}

/// A stored Job ready to run.
#[derive(Debug, Clone)]
pub struct Job {
    /// `job.id`.
    pub id: String,
    /// `job.kind`.
    pub kind: String,
    /// `job.payload`, JSON text.
    pub payload: String,
    /// 1 on the first run.
    pub attempt: u32,
}

/// One `.sql` file, applied once and recorded in `migration`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    /// Owning Module (`kernel` for the Kernel).
    pub module: &'static str,
    /// File name without extension, e.g. `0001_people`. Applied in
    /// registration order within a Module.
    pub name: &'static str,
    /// The SQL (SQLite dialect, valid on D1).
    pub sql: &'static str,
}

/// Merged text catalog: Module defaults, then TOML overrides.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub(crate) default_locale: Option<Locale>,
    texts: HashMap<(String, String), String>,
}

impl Catalog {
    pub(crate) fn insert(&mut self, locale: &str, key: &str, text: &str) {
        self.texts.insert((locale.into(), key.into()), text.into());
    }

    /// Whether `key` has a text in `locale`.
    pub fn has(&self, locale: &Locale, key: &str) -> bool {
        self.texts
            .contains_key(&(locale.0.clone(), key.to_string()))
    }

    /// Whether any Locale has a text for `key`.
    pub(crate) fn knows(&self, key: &str) -> bool {
        self.texts.keys().any(|(_, k)| k == key)
    }

    /// The text for `key` in `locale`; falls back to the default Locale,
    /// then to the key itself (so a missing text is visible, not empty).
    pub fn t(&self, locale: &Locale, key: &str) -> String {
        let get = |l: &str| self.texts.get(&(l.to_string(), key.to_string()));
        get(&locale.0)
            .or_else(|| self.default_locale.as_ref().and_then(|d| get(&d.0)))
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }

    /// Every `(locale, key, text)`, unordered (the Texts page sorts them).
    pub(crate) fn entries(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.texts
            .iter()
            .map(|((l, k), t)| (l.as_str(), k.as_str(), t.as_str()))
    }

    /// The text for `key` in exactly `locale`, without fallback.
    pub(crate) fn exact(&self, locale: &str, key: &str) -> Option<&str> {
        self.texts
            .get(&(locale.to_string(), key.to_string()))
            .map(String::as_str)
    }

    /// The Deployment's default Locale.
    pub fn default_locale(&self) -> Locale {
        self.default_locale.clone().unwrap_or_else(Locale::th)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_default_locale_then_key() {
        let mut c = Catalog {
            default_locale: Some(Locale::th()),
            ..Catalog::default()
        };
        c.insert("th", "a", "ก");
        assert_eq!(c.t(&Locale::en(), "a"), "ก");
        assert_eq!(c.t(&Locale::en(), "b"), "b");
        assert!(c.has(&Locale::th(), "a"));
    }
}
