//! `[[menu.variants]]` (#182): a smaller Menu, and a smaller Mini App
//! home, for the Students whose roster attributes match a condition.
//!
//! ```toml
//! [roster.attributes.status]
//! type = "enum"
//! values = ["current", "graduated"]
//!
//! # Graduates see the announcements only.
//! [[menu.variants]]
//! key = "grad"
//! when = { status = ["graduated"] }
//! entries = ["announcements.latest"]
//! app_links = []
//! ```
//!
//! The first variant whose `when` matches a Student's attributes is
//! theirs ([`MenuConfig::variant_for`]); a Student no variant matches gets
//! the whole Menu. `when` is written like `required_when`: one `enum`
//! attribute and some of its values. `entries` are the Action ids of the
//! Menu entries kept (they stay in Menu order) and `app_links` the paths
//! of the Mini App home links kept; anything not listed is left out. The
//! Kernel's unlink entry stays whenever `[student] self_unlink` is on.
//! `build()` checks that every Action is a Menu entry and every path a
//! Mini App link, and that the variant's Menu key is free
//! ([`crate::Deployment::menu_in`]).

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Map, Value as Json};

use super::{AttributeType, RosterConfig};

/// Longest variant key.
const VARIANT_KEY_MAX_LEN: usize = 8;

/// `[menu]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct MenuConfig {
    /// The variants, first match wins.
    pub variants: Vec<MenuVariant>,
}

/// One `[[menu.variants]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct MenuVariant {
    /// 1 to 8 of `a-z` and `0-9`; part of the
    /// variant Menu's key.
    pub key: String,
    /// One `enum` roster attribute and the values that select this variant.
    pub when: BTreeMap<String, Vec<String>>,
    /// Action ids of the Menu entries kept.
    #[serde(default)]
    pub entries: Vec<String>,
    /// Paths of the Mini App home links kept.
    #[serde(default)]
    pub app_links: Vec<String>,
}

impl MenuVariant {
    /// Whether a Student with `attrs` matches `when`: the attribute holds
    /// one of the listed values (an empty one matches none).
    pub fn matches(&self, attrs: &Map<String, Json>) -> bool {
        self.when.iter().all(|(k, values)| match attrs.get(k) {
            Some(Json::String(s)) => values.iter().any(|v| v == s),
            _ => false,
        })
    }

    /// Whether the Menu entry of Action `action` is kept.
    pub fn keeps_entry(&self, action: &str) -> bool {
        self.entries.iter().any(|a| a == action)
    }

    /// Whether the Mini App home link to `path` is kept.
    pub fn keeps_app_link(&self, path: &str) -> bool {
        self.app_links.iter().any(|p| p == path)
    }

    /// [`MenuVariant::matches`] in SQL over the JSON column `col` (e.g.
    /// `s.attributes`): the text and its values, in order. Never `NULL`.
    pub(crate) fn sql(&self, col: &str) -> (String, Vec<String>) {
        let mut parts = Vec::new();
        let mut binds = Vec::new();
        for (k, values) in &self.when {
            let marks = vec!["?"; values.len()].join(", ");
            parts.push(format!("COALESCE(json_extract({col}, ?), '') IN ({marks})"));
            binds.push(format!("$.\"{k}\""));
            binds.extend(values.iter().cloned());
        }
        (format!("({})", parts.join(" AND ")), binds)
    }
}

impl MenuConfig {
    /// The variant of a Student with `attrs`: the first that matches.
    pub fn variant_for(&self, attrs: &Map<String, Json>) -> Option<&MenuVariant> {
        self.variants.iter().find(|v| v.matches(attrs))
    }

    /// The variant named `key`.
    pub fn variant(&self, key: &str) -> Option<&MenuVariant> {
        self.variants.iter().find(|v| v.key == key)
    }

    /// The attributes some variant's `when` names.
    pub fn attributes(&self) -> BTreeSet<&str> {
        self.variants
            .iter()
            .flat_map(|v| v.when.keys().map(String::as_str))
            .collect()
    }

    /// Refuses a bad or repeated key, and a `when` that does not name one
    /// `enum` attribute of `roster` and some of its values (each once).
    pub(crate) fn check(&self, roster: &RosterConfig) -> crate::Result<()> {
        let mut keys = BTreeSet::new();
        for v in &self.variants {
            let bad = |why: String| {
                crate::Error::Invalid(format!("[[menu.variants]] `{}`: {why}", v.key))
            };
            let ok = !v.key.is_empty()
                && v.key.len() <= VARIANT_KEY_MAX_LEN
                && v.key
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
            if !ok {
                return Err(bad(format!(
                    "key: 1 to {VARIANT_KEY_MAX_LEN} of a-z and 0-9"
                )));
            }
            if !keys.insert(v.key.as_str()) {
                return Err(bad("key is used twice".into()));
            }
            let [(attr, values)] = v.when.iter().collect::<Vec<_>>()[..] else {
                return Err(bad("when names one attribute".into()));
            };
            let Some(spec) = roster.attributes.get(attr) else {
                return Err(bad(format!("when: no attribute `{attr}`")));
            };
            if spec.kind != AttributeType::Enum {
                return Err(bad(format!("when: `{attr}` must be an enum")));
            }
            if values.is_empty() {
                return Err(bad(format!("when: list at least one value of `{attr}`")));
            }
            let mut seen = BTreeSet::new();
            for value in values {
                if !spec.values.contains(value) {
                    return Err(bad(format!("when: `{attr}` has no value `{value}`")));
                }
                if !seen.insert(value) {
                    return Err(bad(format!("when: value `{value}` is listed twice")));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;
