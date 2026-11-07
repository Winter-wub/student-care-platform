//! Audience resolution (#10, #25): which Students and live Links an
//! [`Audience`] (a filter Staff build on the send page) covers, as one SQL
//! query built by [`statement`] and run by [`resolve`].
//!
//! The filters are AND-ed:
//! - **attributes**: for each key (one of `[roster.attributes]`), any of
//!   the listed values (OR), compared as text with the stored JSON value
//!   (`2`, `3.5`, `MSC`);
//! - **tags**: the Student holds any of the listed Tags (by name, roster
//!   or manual);
//! - **hand-picked Students**: [`Audience::students`], student codes (the
//!   roster's student ID). Given with other filters it is one more AND
//!   (the Student must be on the list and match the rest); given alone it
//!   is the whole Audience. An id that does not fit `[student] id_pattern`
//!   or names nobody selects nobody ([`hand_picked`] reports it);
//! - **Link status**: `active` Links only, or also `unreachable` ones
//!   ([`Audience::include_unreachable`]): a sender counts those but skips
//!   them.
//!
//! Only `active` Students are in an Audience (`left` never), and only
//! through a live (not revoked) Link: a Student on two Channels has two
//! [`Recipient`]s.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::RosterConfig;
use crate::delivery::ChannelAccountId;
use crate::message::Locale;
use crate::port_types::{Rows, Statement};
use crate::{Deployment, Error, Result};

/// Most Tags in one Audience.
pub const MAX_TAGS: usize = 20;
/// Most attribute values (over every key) in one Audience.
pub const MAX_VALUES: usize = 50;
/// Most hand-picked student ids in one Audience.
pub const MAX_STUDENTS: usize = 2000;

/// A filter over Students, stored as JSON with what it selected.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Audience {
    /// Attribute key → accepted values (any of). Empty: no attribute filter.
    #[serde(default)]
    pub attributes: BTreeMap<String, Vec<String>>,
    /// Tag names (any of). Empty: no Tag filter.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Hand-picked student codes (any of). Empty: no list. AND-ed with the
    /// other filters; alone it is the whole Audience.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub students: Vec<String>,
    /// Also select `unreachable` Links (counted, then skipped by senders).
    #[serde(default)]
    pub include_unreachable: bool,
}

impl Audience {
    /// Everyone with an active Link.
    pub fn everyone() -> Self {
        Self::default()
    }

    /// Checks the filter against the roster schema: known attribute keys,
    /// non-empty values, at most [`MAX_TAGS`] Tags and [`MAX_VALUES`]
    /// values. `Error::Invalid` names what is wrong (never a value).
    pub fn check(&self, roster: &RosterConfig) -> Result<()> {
        for (key, values) in &self.attributes {
            if !roster.attributes.contains_key(key) {
                return Err(Error::Invalid(format!("unknown attribute `{key}`")));
            }
            if values.is_empty() || values.iter().any(|v| v.trim().is_empty()) {
                return Err(Error::Invalid(format!("empty value for `{key}`")));
            }
        }
        let values: usize = self.attributes.values().map(Vec::len).sum();
        if values > MAX_VALUES {
            return Err(Error::Invalid(format!("more than {MAX_VALUES} values")));
        }
        if self.tags.len() > MAX_TAGS || self.tags.iter().any(|t| t.trim().is_empty()) {
            return Err(Error::Invalid("tags".into()));
        }
        if self.students.len() > MAX_STUDENTS {
            return Err(Error::Invalid(format!(
                "more than {MAX_STUDENTS} student ids"
            )));
        }
        Ok(())
    }

    /// The Audience as an audit row records it: the hand-picked list is
    /// its size (`hand_picked`), never the ids.
    pub fn audit_json(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Some(o) = v.as_object_mut() {
            o.remove("students");
            if !self.students.is_empty() {
                o.insert("hand_picked".into(), self.students.len().into());
            }
        }
        v
    }
}

/// Student ids in `text`, separated by newlines, commas, semicolons or
/// spaces: zero-width characters (U+200B..U+200D, U+FEFF, common in
/// spreadsheet pastes) removed, trimmed, empty ones dropped, duplicates
/// removed (first one kept).
pub fn parse_student_ids(text: &str) -> Vec<String> {
    let clean: String = text
        .chars()
        .filter(|c| !matches!(c, '\u{200B}'..='\u{200D}' | '\u{FEFF}'))
        .collect();
    let mut seen = std::collections::HashSet::new();
    clean
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|id| !id.is_empty() && seen.insert(*id))
        .map(str::to_string)
        .collect()
}

/// One Link an Audience selected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Recipient {
    /// `student.id`.
    pub student_id: String,
    /// The Student's Locale.
    pub locale: Locale,
    /// `link.id`.
    pub link_id: String,
    /// `channel_account.id`.
    pub channel_account_id: String,
    /// The account on its Channel.
    pub account: ChannelAccountId,
    /// The Link is `unreachable` (only with
    /// [`Audience::include_unreachable`]).
    pub unreachable: bool,
}

/// Students and Links per Channel kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Counts {
    /// Distinct Students.
    pub students: usize,
    /// Channel kind → (active Links, unreachable Links).
    pub per_channel: BTreeMap<String, (usize, usize)>,
}

impl Counts {
    /// Counts `recipients`.
    pub fn of(recipients: &[Recipient]) -> Self {
        let mut students: Vec<&str> = recipients.iter().map(|r| r.student_id.as_str()).collect();
        students.sort_unstable();
        students.dedup();
        let mut per_channel: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for r in recipients {
            let e = per_channel
                .entry(r.account.channel.as_str().to_string())
                .or_default();
            if r.unreachable {
                e.1 += 1;
            } else {
                e.0 += 1;
            }
        }
        Self {
            students: students.len(),
            per_channel,
        }
    }
}

/// The query selecting `a`'s Recipients, ordered by Student then Channel.
/// Values are bound, never spliced; attribute keys are checked by
/// [`Audience::check`] and bound as JSON paths.
pub fn statement(a: &Audience, roster: &RosterConfig) -> Result<Statement> {
    a.check(roster)?;
    let statuses = if a.include_unreachable {
        "'active', 'unreachable'"
    } else {
        "'active'"
    };
    let mut sql = format!(
        "SELECT s.id AS student_id, s.locale, l.id AS link_id, l.status, ca.id AS account_id, \
         ca.channel, ca.external_id FROM student s \
         JOIN link l ON l.student_id = s.id AND l.status IN ({statuses}) \
         JOIN channel_account ca ON ca.id = l.channel_account_id \
         WHERE s.status = 'active'"
    );
    let mut params: Vec<String> = Vec::new();
    for (key, values) in &a.attributes {
        sql.push_str(&format!(
            " AND CAST(json_extract(s.attributes, ?) AS TEXT) IN ({})",
            marks(values.len())
        ));
        params.push(format!("$.\"{key}\""));
        params.extend(values.iter().map(|v| v.trim().to_string()));
    }
    if !a.tags.is_empty() {
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM student_tag st JOIN tag t ON t.id = st.tag_id \
             WHERE st.student_id = s.id AND t.name IN ({}))",
            marks(a.tags.len())
        ));
        params.extend(a.tags.iter().map(|t| t.trim().to_string()));
    }
    if !a.students.is_empty() {
        // One JSON bind, not one per id: D1 allows about 100 per query.
        sql.push_str(" AND s.student_code IN (SELECT value FROM json_each(?))");
        params.push(serde_json::to_string(&a.students).unwrap_or_else(|_| "[]".into()));
    }
    sql.push_str(" ORDER BY s.id, ca.channel");
    Ok(params
        .into_iter()
        .fold(Statement::new(sql), |s, p| s.bind(p)))
}

fn marks(n: usize) -> String {
    vec!["?"; n].join(", ")
}

/// Runs [`statement`] on `d`: every Recipient of `a` on a Channel the
/// Deployment has (Links on a Channel since removed are left out).
pub async fn resolve(d: &Deployment, a: &Audience) -> Result<Vec<Recipient>> {
    // The whole list is checked first: ids dropped below still count
    // against the cap.
    a.check(&d.config().roster)?;
    let mut a = std::borrow::Cow::Borrowed(a);
    if !a.students.is_empty() {
        // Ids that do not fit `[student] id_pattern` name nobody. A list
        // with none left selects nobody, never everybody.
        let pattern = d.config().student.id_pattern.as_deref();
        let fit: Vec<String> = a
            .students
            .iter()
            .filter(|id| crate::kernel::flow::fits(id, pattern))
            .cloned()
            .collect();
        if fit.is_empty() {
            return Ok(Vec::new());
        }
        if fit.len() != a.students.len() {
            a.to_mut().students = fit;
        }
    }
    let stmt = statement(&a, &d.config().roster)?;
    let rows = d.services().db.query(stmt).await?;
    Ok(recipients(d, &rows))
}

/// [`resolve`], counted.
pub async fn count(d: &Deployment, a: &Audience) -> Result<Counts> {
    Ok(Counts::of(&resolve(d, a).await?))
}

fn recipients(d: &Deployment, rows: &Rows) -> Vec<Recipient> {
    let text = |i: usize, c: &str| {
        rows.get(i, c)
            .and_then(|v| v.as_text())
            .unwrap_or_default()
            .to_string()
    };
    (0..rows.len())
        .filter_map(|i| {
            let channel = d.channel(&text(i, "channel"))?.kind();
            Some(Recipient {
                student_id: text(i, "student_id"),
                locale: Locale::new(text(i, "locale")),
                link_id: text(i, "link_id"),
                channel_account_id: text(i, "account_id"),
                account: ChannelAccountId::new(channel, text(i, "external_id")),
                unreachable: text(i, "status") == "unreachable",
            })
        })
        .collect()
}

/// What became of a hand-picked list ([`Audience::students`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct HandPicked {
    /// Ids that fit no Student: a wrong pattern or not on the roster.
    pub unknown: Vec<String>,
    /// Students on the roster with no live Link (a `left` Student has
    /// none): listed, never sent to. With
    /// [`Audience::include_unreachable`] an `unreachable` Link is live.
    pub unlinked: Vec<String>,
    /// Distinct Students on the list that do have a live Link.
    pub linked: usize,
}

/// Sorts `a.students` into [`HandPicked`], in list order, ignoring the
/// other filters (the count of [`resolve`] shows what they leave).
pub async fn hand_picked(d: &Deployment, a: &Audience) -> Result<HandPicked> {
    if a.students.len() > MAX_STUDENTS {
        return Err(Error::Invalid(format!(
            "more than {MAX_STUDENTS} student ids"
        )));
    }
    let pattern = d.config().student.id_pattern.as_deref();
    let (fit, mut unknown): (Vec<&String>, Vec<&String>) = a
        .students
        .iter()
        .partition(|id| crate::kernel::flow::fits(id, pattern));
    let mut out = HandPicked::default();
    if !fit.is_empty() {
        let stmt = Statement::new(
            "SELECT id, student_code FROM student WHERE student_code IN \
             (SELECT value FROM json_each(?))",
        )
        .bind(serde_json::to_string(&fit).unwrap_or_else(|_| "[]".into()));
        let rows = d.services().db.query(stmt).await?;
        let found: BTreeMap<String, String> = (0..rows.len())
            .filter_map(|i| {
                let t = |c: &str| rows.get(i, c).and_then(|v| v.as_text()).map(str::to_string);
                Some((t("student_code")?, t("id")?))
            })
            .collect();
        let mut only = Audience::everyone();
        only.students = found.keys().cloned().collect();
        only.include_unreachable = a.include_unreachable;
        let live: std::collections::HashSet<String> = resolve(d, &only)
            .await?
            .into_iter()
            .map(|r| r.student_id)
            .collect();
        for id in fit {
            match found.get(id) {
                None => unknown.push(id),
                Some(sid) if live.contains(sid) => out.linked += 1,
                Some(_) => out.unlinked.push(id.clone()),
            }
        }
    }
    // Keep the order of the list.
    let rank: BTreeMap<&String, usize> =
        a.students.iter().enumerate().map(|(i, s)| (s, i)).collect();
    unknown.sort_by_key(|id| rank[id]);
    out.unknown = unknown.into_iter().cloned().collect();
    Ok(out)
}
