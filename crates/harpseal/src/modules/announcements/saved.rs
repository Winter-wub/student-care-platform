//! Saved Audiences (#144, Module migration `0004_saved_audiences`): an
//! Audience Staff named on the editor, kept as its rules (the choice and
//! the filters, [`Saved`]), never as the Students it selected. Each write
//! is one batch: an audit row written only if the write can happen (the
//! name free, the row still there), then the change gated on that row.
//!
//! The `audience` column is the JSON of [`Audience`] plus `"v": 1`
//! ([`VERSION`]). A row whose JSON does not read, or that carries another
//! version, is unreadable ([`Saved::audience`] is `None`): it is never
//! read as everyone, never used, only deleted. A later change to the shape
//! bumps [`VERSION`] and reads the old one explicitly; a field added with
//! `#[serde(default)]` needs no bump.

use serde_json::json;

use super::gate::GATED;
use super::store::{int, text};
use crate::audience::Audience;
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::config::{AttributeType, RosterConfig};
use crate::port_types::{Rows, Statement};
use crate::{Result, Services};

/// Longest name, in characters.
pub(crate) const MAX_NAME: usize = 60;
/// Most saved Audiences: the editor's picker stays one short list.
pub(crate) const MAX_SAVED: i64 = 100;
/// Audit target kind of a saved Audience.
const KIND: &str = "saved_audience";
/// The version of the stored JSON (`"v"`).
pub(crate) const VERSION: i64 = 1;
/// The choices a saved Audience may keep (the column's `CHECK`).
pub(crate) const SCOPES: [&str; 4] = ["", "everyone", "groups", "students"];

/// One `saved_audience` row.
#[derive(Debug, Clone)]
pub(crate) struct Saved {
    pub(crate) id: String,
    pub(crate) name: String,
    /// `everyone`, `groups`, `students`, or `""` (groups AND a list).
    pub(crate) scope: String,
    /// The rules; `None` when the stored JSON does not read (see the
    /// module docs): shown as unreadable, only deleted.
    pub(crate) audience: Option<Audience>,
    pub(crate) updated_at: i64,
}

/// Why a write did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    /// Another saved Audience has the name.
    Taken,
    /// The saved Audience is gone.
    Gone,
    /// [`MAX_SAVED`] are saved already.
    Full,
}

const COLUMNS: &str = "id, name, scope, audience, updated_at";

fn row_at(rows: &Rows, i: usize) -> Saved {
    Saved {
        id: text(rows, i, "id"),
        name: text(rows, i, "name"),
        scope: text(rows, i, "scope"),
        audience: decode(&text(rows, i, "audience")),
        updated_at: int(rows, i, "updated_at").unwrap_or(0),
    }
}

/// `a` as stored: its JSON with `"v"`.
pub(crate) fn encode(a: &Audience) -> String {
    let mut v = serde_json::to_value(a).unwrap_or_else(|_| json!({}));
    if let Some(o) = v.as_object_mut() {
        o.insert("v".into(), VERSION.into());
    }
    v.to_string()
}

/// The stored JSON as an Audience: `None` unless it parses as an object
/// of version [`VERSION`] whose fields have their types.
pub(crate) fn decode(stored: &str) -> Option<Audience> {
    let v: serde_json::Value = serde_json::from_str(stored).ok()?;
    if v.get("v").and_then(serde_json::Value::as_i64) != Some(VERSION) {
        return None;
    }
    serde_json::from_value(v).ok()
}

/// Every saved Audience, by name.
pub(crate) async fn list(s: &Services) -> Result<Vec<Saved>> {
    let sql = format!("SELECT {COLUMNS} FROM saved_audience ORDER BY name, id");
    let rows = s.db.query(Statement::new(sql)).await?;
    Ok((0..rows.len()).map(|i| row_at(&rows, i)).collect())
}

/// The saved Audience `id`, if it exists.
pub(crate) async fn load(s: &Services, id: &str) -> Result<Option<Saved>> {
    let sql = format!("SELECT {COLUMNS} FROM saved_audience WHERE id = ?");
    let rows = s.db.query(Statement::new(sql).bind(id)).await?;
    Ok((!rows.is_empty()).then(|| row_at(&rows, 0)))
}

/// `name` without zero-width characters (U+200B..U+200D, U+FEFF, as
/// [`crate::audience::parse_student_ids`] drops them), trimmed, with inner
/// runs of white space as one space; `None` when empty or longer than
/// [`MAX_NAME`].
pub(crate) fn clean_name(name: &str) -> Option<String> {
    let name: String = name
        .chars()
        .filter(|c| !matches!(c, '\u{200B}'..='\u{200D}' | '\u{FEFF}'))
        .collect();
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let len = name.chars().count();
    (1..=MAX_NAME).contains(&len).then_some(name)
}

/// Filters of `a` that `[roster]` no longer has, as (attribute key,
/// value): an unknown key (with each of its values), or a value an enum
/// attribute no longer lists. Such a filter is shown and refused, never
/// dropped: dropping it would widen the Audience.
pub(crate) fn stale(a: &Audience, roster: &RosterConfig) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (key, values) in &a.attributes {
        let spec = roster.attributes.get(key);
        for value in values {
            let gone = match spec {
                None => true,
                Some(s) => s.kind == AttributeType::Enum && !s.values.contains(value),
            };
            if gone {
                out.push((key.clone(), value.clone()));
            }
        }
    }
    out
}

fn after(name: &str, scope: &str, a: &Audience) -> serde_json::Value {
    json!({ "name": name, "scope": scope, "audience": a.audit_json() })
}

/// Saves `a` under `name` (already [`clean_name`]d); returns its id.
/// Audit `announcement.audience_save`.
pub(crate) async fn create(
    s: &Services,
    name: &str,
    scope: &str,
    a: &Audience,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<std::result::Result<String, Refused>> {
    let id = s.new_id()?;
    let now = s.now_ms();
    let condition = Statement::new(
        "NOT EXISTS (SELECT 1 FROM saved_audience WHERE name = ?) \
         AND (SELECT COUNT(*) FROM saved_audience) < ?",
    )
    .bind(name)
    .bind(MAX_SAVED);
    let (entry, gate) = audit::entry_if(
        s,
        actor,
        "announcement.audience_save",
        Some(&Target::new(KIND, &id)),
        None,
        Some(after(name, scope, a)),
        ctx,
        condition,
    )?;
    let created_by = match actor {
        Actor::Staff(id) => Some(id.clone()),
        _ => None,
    };
    let insert = Statement::new(format!(
        "INSERT INTO saved_audience (id, name, scope, audience, created_by, created_at, \
         updated_at) SELECT ?, ?, ?, ?, ?, ?, ? WHERE {GATED}"
    ))
    .bind(id.as_str())
    .bind(name)
    .bind(scope)
    .bind(encode(a))
    .bind(created_by)
    .bind(now)
    .bind(now)
    .bind(gate.as_str());
    let results = s.db.batch(vec![entry, insert]).await?;
    if results.first().map(|r| r.rows_affected) == Some(1) {
        return Ok(Ok(id));
    }
    let taken =
        s.db.query(Statement::new("SELECT 1 FROM saved_audience WHERE name = ?").bind(name))
            .await?;
    Ok(Err(if taken.is_empty() {
        Refused::Full
    } else {
        Refused::Taken
    }))
}

/// Renames `old` to `name` (already [`clean_name`]d). Audit
/// `announcement.audience_rename`.
pub(crate) async fn rename(
    s: &Services,
    old: &Saved,
    name: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<std::result::Result<(), Refused>> {
    let condition = Statement::new(
        "EXISTS (SELECT 1 FROM saved_audience WHERE id = ?) \
         AND NOT EXISTS (SELECT 1 FROM saved_audience WHERE name = ? AND id <> ?)",
    )
    .bind(old.id.as_str())
    .bind(name)
    .bind(old.id.as_str());
    let (entry, gate) = audit::entry_if(
        s,
        actor,
        "announcement.audience_rename",
        Some(&Target::new(KIND, &old.id)),
        Some(json!({ "name": old.name })),
        Some(json!({ "name": name })),
        ctx,
        condition,
    )?;
    let update = Statement::new(format!(
        "UPDATE saved_audience SET name = ?, updated_at = ? WHERE id = ? AND {GATED}"
    ))
    .bind(name)
    .bind(s.now_ms())
    .bind(old.id.as_str())
    .bind(gate.as_str());
    let results = s.db.batch(vec![entry, update]).await?;
    if results.first().map(|r| r.rows_affected) == Some(1) {
        return Ok(Ok(()));
    }
    Ok(Err(if load(s, &old.id).await?.is_some() {
        Refused::Taken
    } else {
        Refused::Gone
    }))
}

/// Deletes `old`. Announcements that used it keep their own copy of the
/// Audience. Audit `announcement.audience_delete`.
pub(crate) async fn delete(
    s: &Services,
    old: &Saved,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<std::result::Result<(), Refused>> {
    let condition =
        Statement::new("EXISTS (SELECT 1 FROM saved_audience WHERE id = ?)").bind(old.id.as_str());
    let (entry, gate) = audit::entry_if(
        s,
        actor,
        "announcement.audience_delete",
        Some(&Target::new(KIND, &old.id)),
        Some(match &old.audience {
            Some(a) => after(&old.name, &old.scope, a),
            None => json!({ "name": old.name, "scope": old.scope, "unreadable": true }),
        }),
        None,
        ctx,
        condition,
    )?;
    let delete = Statement::new(format!(
        "DELETE FROM saved_audience WHERE id = ? AND {GATED}"
    ))
    .bind(old.id.as_str())
    .bind(gate.as_str());
    let results = s.db.batch(vec![entry, delete]).await?;
    if results.first().map(|r| r.rows_affected) == Some(1) {
        Ok(Ok(()))
    } else {
        Ok(Err(Refused::Gone))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_lose_zero_width_characters() {
        assert_eq!(
            clean_name(" ปี\u{200B}สอง \u{FEFF} MSC "),
            Some("ปีสอง MSC".into())
        );
        assert_eq!(clean_name("\u{200B}\u{200C}\u{200D}\u{FEFF} "), None);
        assert_eq!(clean_name(&"ก".repeat(MAX_NAME + 1)), None);
    }

    #[test]
    fn only_the_known_version_reads() {
        let mut a = Audience::everyone();
        a.tags = vec!["ชมรม".into()];
        assert_eq!(decode(&encode(&a)), Some(a));
        assert_eq!(decode(r#"{"tags":["x"]}"#), None, "no version");
        assert_eq!(decode(r#"{"v":2,"tags":["x"]}"#), None);
        assert_eq!(
            decode(r#"{"v":1,"tags":"x"}"#),
            None,
            "a field of another type"
        );
        assert_eq!(decode("{not json"), None);
    }
}
