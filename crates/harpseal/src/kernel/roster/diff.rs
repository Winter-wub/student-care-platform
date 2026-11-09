//! What an import changes, Student by Student (#134). Computed once, when
//! the file is previewed (the Students are already loaded to compare it),
//! never on a page view. Both go to the FileStore, never into the
//! `roster_import` row (whose `counts` the Job, the polls and the prune
//! read, and which D1 caps at 2 MB): the whole list as JSON Lines under
//! [`key`], which `changes.csv` reads, and the first [`SHOWN_CHANGES`]
//! changes and [`SHOWN_LEAVING`] leavers, each value cut to
//! [`SHOWN_CHARS`] characters, as one small [`Head`] under [`head_key`],
//! which only the preview page reads. A change holds the field and its
//! stored values (`before`, `after`); the page and the CSV label them in
//! the Staff member's Locale with the `[roster]` labels. Both objects are
//! deleted with the file after a year (`prune.rs`); `changes.csv` is then
//! gone (410).

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use super::store::{Existing, Tally};
use super::validate::Record;
use crate::port_types::{Statement, Value};
use crate::students::Merge;
use crate::{Error, Result, Services};

/// Changes of listed Students kept in the preview, in file order.
pub(crate) const SHOWN_CHANGES: usize = 100;
/// Students who will become left kept in the preview, by student code.
pub(crate) const SHOWN_LEAVING: usize = 200;

/// Longest value the page shows inline, in characters (a long text
/// attribute is cut, with `…`); the CSV keeps it whole.
pub(crate) const SHOWN_CHARS: usize = 200;

/// The FileStore key of import `id`'s full list of changes.
pub(crate) fn key(id: &str) -> String {
    format!("roster-diff/{id}.jsonl")
}

/// The FileStore key of import `id`'s [`Head`].
pub(crate) fn head_key(id: &str) -> String {
    format!("roster-diff/{id}.head.json")
}

/// What the preview page lists: the first changes of listed Students, in
/// file order, and the first leavers, by student code.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Head {
    pub(crate) changes: Vec<Change>,
    pub(crate) leaving: Vec<Change>,
}

impl Head {
    /// The head of `all` (changes, then leavers), values cut.
    pub(crate) fn of(all: &[Change]) -> Self {
        let (leaving, changes): (Vec<&Change>, Vec<&Change>) =
            all.iter().partition(|c| c.kind == "leaving");
        Self {
            changes: changes.into_iter().take(SHOWN_CHANGES).map(cut).collect(),
            leaving: leaving.into_iter().take(SHOWN_LEAVING).map(cut).collect(),
        }
    }
}

/// `c` with every text cut to [`SHOWN_CHARS`].
fn cut(c: &Change) -> Change {
    let mut c = c.clone();
    c.name = cut_text(&c.name);
    for f in &mut c.fields {
        cut_json(&mut f.before);
        cut_json(&mut f.after);
    }
    c
}

fn cut_text(t: &str) -> String {
    if t.chars().count() <= SHOWN_CHARS {
        return t.to_string();
    }
    let mut out: String = t.chars().take(SHOWN_CHARS).collect();
    out.push('…');
    out
}

fn cut_json(x: &mut Json) {
    match x {
        Json::String(t) => *t = cut_text(t),
        Json::Array(a) => a.iter_mut().for_each(cut_json),
        _ => {}
    }
}

/// Stores `all` (nothing when empty): the list, then its head.
pub(crate) async fn store(s: &Services, id: &str, all: &[Change]) -> Result<()> {
    if all.is_empty() {
        return Ok(());
    }
    let json = "application/jsonl";
    s.files.put(&key(id), encode(all)?, json).await?;
    let head =
        serde_json::to_vec(&Head::of(all)).map_err(|_| Error::Invalid("roster changes".into()))?;
    s.files.put(&head_key(id), head, "application/json").await
}

/// The head of import `id`, `None` when none is stored (nothing changes,
/// or pruned) or it no longer reads.
pub(crate) async fn head(s: &Services, id: &str) -> Result<Option<Head>> {
    Ok(s.files
        .get(&head_key(id))
        .await?
        .and_then(|b| serde_json::from_slice(&b).ok()))
}

/// Deletes both objects of import `id` (a missing one is no error).
pub(crate) async fn delete(s: &Services, id: &str) -> Result<()> {
    s.files.delete(&key(id)).await?;
    s.files.delete(&head_key(id)).await
}

/// What happens to one Student.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Change {
    pub(crate) code: String,
    /// The name after the import (the stored one for a leaver).
    pub(crate) name: String,
    /// `new`, `updated`, `returning` or `leaving`.
    pub(crate) kind: String,
    /// The fields that change (none for `new` and `leaving`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) fields: Vec<FieldChange>,
    /// Live chat Links the import revokes (`leaving` only).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) links: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// One field of a Student, before and after the import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FieldChange {
    /// `display_name`, `email`, `locale`, `status`, `tags` or
    /// `attributes.<key>`.
    pub(crate) field: String,
    /// The stored value: `null` when none; Tags are a list.
    #[serde(default)]
    pub(crate) before: Json,
    #[serde(default)]
    pub(crate) after: Json,
}

/// The change `rec` makes, from what [`super::store::classify`] decided;
/// `None` when it changes nothing.
pub(crate) fn change(
    existing: Option<&Existing>,
    rec: &Record,
    t: &Tally,
    m: &Merge,
) -> Option<Change> {
    let kind = if t.new > 0 {
        "new"
    } else if t.returning > 0 {
        "returning"
    } else if t.updated > 0 {
        "updated"
    } else {
        return None;
    };
    Some(Change {
        code: rec.code.clone(),
        name: m.fields.display_name.clone(),
        kind: kind.into(),
        fields: existing.map(|e| fields(e, m, rec)).unwrap_or_default(),
        links: 0,
    })
}

/// A Student a `full` import makes left, with their live Links.
pub(crate) fn leaving(code: &str, e: &Existing, links: &HashMap<String, u64>) -> Change {
    Change {
        code: code.into(),
        name: e.fields.display_name.clone(),
        kind: "leaving".into(),
        fields: Vec::new(),
        links: links.get(&e.id).copied().unwrap_or(0),
    }
}

/// The fields that differ between the stored Student and the merge.
fn fields(e: &Existing, m: &Merge, rec: &Record) -> Vec<FieldChange> {
    let mut out = Vec::new();
    let mut push = |field: String, before: Json, after: Json| {
        if before != after {
            out.push(FieldChange {
                field,
                before,
                after,
            });
        }
    };
    for f in ["display_name", "email", "locale", "status"] {
        push(f.into(), e.fields.value_of(f), m.fields.value_of(f));
    }
    let keys: BTreeSet<&String> = e
        .fields
        .attributes
        .keys()
        .chain(m.fields.attributes.keys())
        .collect();
    for k in keys {
        let f = format!("attributes.{k}");
        push(f.clone(), e.fields.value_of(&f), m.fields.value_of(&f));
    }
    let mut tags = rec.tags.clone();
    tags.sort();
    push(
        "tags".into(),
        Json::from(e.roster_tags.clone()),
        Json::from(tags),
    );
    out
}

/// Live (active or unreachable) Links per Student id, in one query: what
/// the left phase would revoke (`links::live_links_of_students`).
pub(crate) async fn live_links(s: &Services) -> Result<HashMap<String, u64>> {
    let rows =
        s.db.query(Statement::new(
            "SELECT student_id, COUNT(*) AS n FROM link WHERE status <> 'revoked' \
             GROUP BY student_id",
        ))
        .await?;
    Ok((0..rows.len())
        .filter_map(|i| {
            let id = rows.get(i, "student_id").and_then(Value::as_text)?;
            let n = rows.get(i, "n").and_then(Value::as_i64)?;
            Some((id.to_string(), u64::try_from(n).unwrap_or(0)))
        })
        .collect())
}

/// The changes as JSON Lines.
pub(crate) fn encode(changes: &[Change]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for c in changes {
        serde_json::to_writer(&mut out, c).map_err(|_| Error::Invalid("roster changes".into()))?;
        out.push(b'\n');
    }
    Ok(out)
}

/// The changes stored by [`encode`]; a line that no longer reads is
/// skipped rather than failing the whole download.
pub(crate) fn decode(bytes: &[u8]) -> Vec<Change> {
    bytes
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_slice(l).ok())
        .collect()
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
