//! `deployment.toml`: data-only configuration beside the binary.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::manifest::Catalog;
use crate::message::Locale;

#[derive(Debug, Clone, Deserialize)]
pub struct DeploymentConfig {
    pub name: String,
    pub default_locale: Locale,
    /// Initial Roles: role name -> Permission ids.
    #[serde(default)]
    pub roles: BTreeMap<String, Vec<String>>,
    pub office_hours: OfficeHours,
    /// Quota per metered thing, e.g. `line_push_per_month = 300`.
    #[serde(default)]
    pub quotas: BTreeMap<String, u64>,
    /// locale -> key -> text; overrides Module catalog defaults.
    #[serde(default)]
    pub texts: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OfficeHours {
    pub timezone: String,
    /// day (mon..sun) -> ["08:30-16:30", ...]
    pub days: BTreeMap<String, Vec<String>>,
}

impl DeploymentConfig {
    pub fn from_toml(src: &str) -> crate::Result<Self> {
        toml::from_str(src).map_err(|e| crate::Error::Invalid(e.to_string()))
    }

    /// Applies `texts` on top of a catalog already holding Module defaults.
    pub fn apply_texts(&self, catalog: &mut Catalog) {
        catalog.default_locale = Some(self.default_locale.clone());
        for (locale, entries) in &self.texts {
            for (k, v) in entries {
                catalog.insert(locale, k, v);
            }
        }
    }
}
