//! Audience resolution (#10, #25): which Students and live Links an
//! [`Audience`] (a filter Staff build on the send page) covers, as one SQL
//! query built by [`statement`] and run by [`resolve`].
//!
//! The filters are AND-ed:
//! - **attributes**: for each key (one of `[roster.attributes]`), any of
//!   the listed values (OR), compared as text with the stored JSON value
//!   (`2`, `3.5`, `DDS`);
//! - **tags**: the Student holds any of the listed Tags (by name, roster
//!   or manual);
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
        Ok(())
    }
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
    let stmt = statement(a, &d.config().roster)?;
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
