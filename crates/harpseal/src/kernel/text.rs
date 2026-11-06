//! Texts at run time: the merged [`Catalog`] (Module defaults, then
//! `[texts]` in the TOML) with `setting` rows on top, read once per
//! Inbound. Also the current Consent notice version, which lives in the
//! same table (key `kernel.consent.version`, Locale `''`).

use std::collections::HashMap;
use std::sync::Arc;

use crate::manifest::Catalog;
use crate::message::Locale;
use crate::port_types::Statement;
use crate::{Result, Services};

/// `setting` key of the notice version (Locale `''`).
pub(crate) const CONSENT_VERSION_KEY: &str = "kernel.consent.version";

/// Every text of one Inbound, in one Locale.
#[derive(Clone)]
pub(crate) struct Texts {
    catalog: Arc<Catalog>,
    /// (locale, key) -> value from `setting`.
    overrides: Arc<HashMap<(String, String), String>>,
    locale: Locale,
}

impl Texts {
    /// Reads every `setting` row (a small table: texts and one version).
    pub(crate) async fn load(
        services: &Services,
        catalog: Arc<Catalog>,
        locale: Locale,
    ) -> Result<Self> {
        let rows = services
            .db
            .query(Statement::new("SELECT key, locale, value FROM setting"))
            .await?;
        let overrides = rows
            .rows
            .iter()
            .filter_map(|r| match (r.first(), r.get(1), r.get(2)) {
                (Some(k), Some(l), Some(v)) => Some((
                    (l.as_text()?.to_string(), k.as_text()?.to_string()),
                    v.as_text()?.to_string(),
                )),
                _ => None,
            })
            .collect();
        Ok(Self {
            catalog,
            overrides: Arc::new(overrides),
            locale,
        })
    }

    /// The same texts in another Locale.
    pub(crate) fn in_locale(&self, locale: &Locale) -> Self {
        Self {
            locale: locale.clone(),
            ..self.clone()
        }
    }

    pub(crate) fn locale(&self) -> &Locale {
        &self.locale
    }

    fn setting(&self, locale: &str, key: &str) -> Option<&String> {
        self.overrides.get(&(locale.to_string(), key.to_string()))
    }

    /// `key` in this Locale: `setting`, then the catalog in this Locale,
    /// then `setting` in the default Locale, then the catalog's fallback.
    pub(crate) fn t(&self, key: &str) -> String {
        if let Some(v) = self.setting(self.locale.as_str(), key) {
            return v.clone();
        }
        if self.catalog.has(&self.locale, key) {
            return self.catalog.t(&self.locale, key);
        }
        let default = self.catalog.default_locale();
        match self.setting(default.as_str(), key) {
            Some(v) => v.clone(),
            None => self.catalog.t(&self.locale, key),
        }
    }

    /// [`Texts::t`] with `{name}` placeholders replaced.
    pub(crate) fn tf(&self, key: &str, args: &[(&str, &str)]) -> String {
        fill(&self.t(key), args)
    }

    /// The current notice version: `setting`, else `[consent] version`.
    pub(crate) fn consent_version(&self, configured: &str) -> String {
        self.setting("", CONSENT_VERSION_KEY)
            .cloned()
            .unwrap_or_else(|| configured.to_string())
    }
}

/// Replaces each `{name}` in `text`.
pub(crate) fn fill(text: &str, args: &[(&str, &str)]) -> String {
    args.iter().fold(text.to_string(), |acc, (k, v)| {
        acc.replace(&format!("{{{k}}}"), v)
    })
}

#[cfg(test)]
mod tests {
    use super::fill;

    #[test]
    fn placeholders_are_replaced_by_name() {
        assert_eq!(
            fill("{a} and {b}, {a}", &[("a", "1"), ("b", "2")]),
            "1 and 2, 1"
        );
        assert_eq!(fill("{unknown}", &[("a", "1")]), "{unknown}");
    }
}
