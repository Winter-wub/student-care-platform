//! What a Module contributes, as data. Shared by both designs; how it gets
//! to the Core (registrar calls vs associated consts) is what differs.

use std::collections::HashMap;

use crate::message::{ActionId, Locale};

/// One Permission the Module defines. Ids are `module.verb`, global.
#[derive(Debug, Clone, Copy)]
pub struct Permission {
    pub id: &'static str,
    pub description: &'static str,
}

/// A Menu entry: label is a catalog key, action must be ≤ 64 bytes.
#[derive(Debug, Clone)]
pub struct MenuEntrySpec {
    pub label_key: &'static str,
    pub action: ActionId,
    /// Order in the Menu; lower first.
    pub order: u16,
}

/// A default bot text. Deployment TOML `texts` overrides it.
#[derive(Debug, Clone, Copy)]
pub struct CatalogEntry {
    pub key: &'static str,
    pub locale: &'static str,
    pub text: &'static str,
}

/// A kind of Job. `schedule` is a cron expression for recurring Jobs.
#[derive(Debug, Clone, Copy)]
pub struct JobSpec {
    pub kind: &'static str,
    pub schedule: Option<&'static str>,
}

/// A stored Job ready to run.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub payload: String,
    pub attempt: u32,
}

/// One `.sql` file, applied once in (module, name) order.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub module: &'static str,
    pub name: &'static str,
    pub sql: &'static str,
}

/// Merged text catalog: Module defaults, then TOML overrides.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub default_locale: Option<Locale>,
    texts: HashMap<(String, String), String>,
}

impl Catalog {
    pub fn insert(&mut self, locale: &str, key: &str, text: &str) {
        self.texts.insert((locale.into(), key.into()), text.into());
    }

    /// Falls back to the default Locale, then to the key itself.
    pub fn t(&self, locale: &Locale, key: &str) -> String {
        let get = |l: &str| self.texts.get(&(l.to_string(), key.to_string()));
        get(&locale.0)
            .or_else(|| self.default_locale.as_ref().and_then(|d| get(&d.0)))
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }
}
