//! The create / edit form's content as the service takes it ([`Draft`]),
//! its audit summary, and the text rows it writes.

use serde_json::{Value as Json, json};

use super::gate::GATED;
use super::store;
use crate::audience::Audience;
use crate::port_types::Statement;

/// What Staff fill in on the create / edit form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Draft {
    /// (locale, title, body); Thai first and required.
    pub(crate) texts: Vec<(String, String, String)>,
    pub(crate) audience: Audience,
    pub(crate) require_ack: bool,
    pub(crate) expires_at: Option<i64>,
    /// `None`: send at once when confirmed.
    pub(crate) send_at: Option<i64>,
    /// Editing a sent announcement: notify its recipients on Channels that
    /// cannot edit (LINE) with one Push each (#77). Ignored otherwise.
    pub(crate) notify: bool,
}

impl Draft {
    pub(super) fn summary(&self) -> Json {
        let mut j = json!({
            "texts": self.texts.iter().map(|(l, t, _)| json!({"locale": l, "title": t})).collect::<Vec<_>>(),
            "audience": self.audience.audit_json(),
            "require_ack": self.require_ack,
            "expires_at": self.expires_at,
            "send_at": self.send_at,
        });
        if self.notify {
            j["notify"] = true.into();
        }
        j
    }
}

pub(super) fn text_rows(id: &str, d: &Draft, gate: Option<&str>) -> Vec<Statement> {
    d.texts
        .iter()
        .map(|(l, t, b)| {
            let sql = match gate {
                None => "INSERT INTO announcement_text (announcement_id, locale, title, body) \
                         VALUES (?, ?, ?, ?)"
                    .to_string(),
                Some(_) => format!(
                    "INSERT INTO announcement_text (announcement_id, locale, title, body) \
                     SELECT ?, ?, ?, ? WHERE {GATED}"
                ),
            };
            let s = Statement::new(sql)
                .bind(id)
                .bind(l.as_str())
                .bind(t.as_str())
                .bind(b.as_str());
            match gate {
                Some(g) => s.bind(g),
                None => s,
            }
        })
        .collect()
}

/// Locales whose title or body differs between `old` and `draft` (added
/// and removed ones included).
pub(super) fn changed_locales(old: &store::Texts, draft: &Draft) -> Vec<String> {
    let new: std::collections::BTreeMap<&str, (&str, &str)> = draft
        .texts
        .iter()
        .map(|(l, t, b)| (l.as_str(), (t.as_str(), b.as_str())))
        .collect();
    let mut keys: Vec<&str> = old
        .0
        .keys()
        .map(String::as_str)
        .chain(new.keys().copied())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .filter(|l| {
            let before = old.0.get(*l).map(|(t, b)| (t.as_str(), b.as_str()));
            before != new.get(l).copied()
        })
        .map(str::to_string)
        .collect()
}
