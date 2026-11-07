//! `[roster]` checks at build time: what `DeploymentConfig::from_toml`
//! (and a config changed in code) refuses.

use super::{AttributeType, RosterConfig, WARNING_CLASSES, rules};
use std::collections::BTreeSet;

use crate::message::Locale;

impl RosterConfig {
    /// Refuses bad keys (not `[a-z][a-z0-9_]*`, at most 40 bytes), empty or
    /// repeated column headers, an `enum` without values (or with empty or
    /// repeated ones), `values` on a non-`enum`, an empty separator, no
    /// `modes` (or one that is not `full` / `upsert`, or repeated) and a
    /// `default_mode` outside `modes`, a `blocking` entry that is not a
    /// warning class (or is repeated), and an attribute whose `min` / `max`
    /// / `value_labels` break the rules of [`rules::check`] (#81).
    pub(crate) fn check(
        &self,
        default_locale: &Locale,
        known: &BTreeSet<String>,
    ) -> crate::Result<()> {
        let bad = |m: String| Err(crate::Error::Invalid(format!("[roster] {m}")));
        let mut columns = vec![&self.student_code, &self.display_name, &self.email];
        columns.extend(self.locale.as_ref().map(|l| &l.column));
        columns.extend(self.tags.as_ref().map(|t| &t.column));
        if self.tags.as_ref().is_some_and(|t| t.separator.is_empty()) {
            return bad("tags separator is empty".into());
        }
        if self.modes.is_empty() {
            return bad("modes is empty".into());
        }
        for (i, m) in self.modes.iter().enumerate() {
            if !matches!(m.as_str(), "full" | "upsert") {
                return bad(format!("mode `{m}` must be `full` or `upsert`"));
            }
            if self.modes[..i].contains(m) {
                return bad(format!("mode `{m}` is listed twice"));
            }
        }
        if !self.allows_mode(&self.default_mode) {
            let d = &self.default_mode;
            return bad(format!("default_mode `{d}` is not in modes"));
        }
        for (i, c) in self.blocking.iter().enumerate() {
            if !WARNING_CLASSES.contains(&c.as_str()) {
                let all = WARNING_CLASSES.join("`, `");
                return bad(format!("blocking `{c}` must be one of `{all}`"));
            }
            if self.blocking[..i].contains(c) {
                return bad(format!("blocking `{c}` is listed twice"));
            }
        }
        for (key, a) in &self.attributes {
            let ok_key = key.len() <= 40
                && key.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
                && key
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if !ok_key {
                return bad(format!("attribute key `{key}` must match [a-z][a-z0-9_]*"));
            }
            let is_enum = a.kind == AttributeType::Enum;
            if is_enum && a.values.is_empty() {
                return bad(format!("attribute `{key}`: enum needs values"));
            }
            if !is_enum && !a.values.is_empty() {
                return bad(format!("attribute `{key}`: values only fit an enum"));
            }
            let mut seen = std::collections::BTreeSet::new();
            if a.values
                .iter()
                .any(|v| v.trim().is_empty() || !seen.insert(v))
            {
                return bad(format!("attribute `{key}`: empty or repeated enum value"));
            }
            if let Err(m) = rules::check(a, default_locale, known) {
                return bad(format!("attribute `{key}`: {m}"));
            }
            columns.push(&a.column);
        }
        let mut seen = std::collections::BTreeSet::new();
        for c in columns {
            if c.trim().is_empty() || !seen.insert(c.trim()) {
                return bad(format!("column header `{c}` is empty or used twice"));
            }
        }
        Ok(())
    }
}
