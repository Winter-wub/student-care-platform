//! Students with missing data (#71): no email, an email that is not valid,
//! or an attribute of `[roster.attributes]` with no value. The roster
//! import warns about these and imports the row anyway, so Staff find
//! them again here: a badge on the Students list and a filter.
//!
//! The filter is one `WHERE` on the page's query. "No email" and "no
//! value" are plain SQL; "not valid" is the roster's address check
//! (`validate::is_email`), which SQL cannot express, so the filter first
//! reads the Students' addresses once (two short columns) and passes the
//! ids of the invalid ones in as one JSON array. No query per Student.
//!
//! **Cost:** a filtered page reads the Student table about three times
//! (the addresses, the count, the page), which D1 bills per row read. The
//! filter is an occasional Staff action, so this is accepted rather than
//! storing an "email invalid" flag that every writer of `student.email`
//! (import, edit form, code) would have to keep in step, and a migration.
//! Revisit if a Deployment's roster grows past some tens of thousands.

use serde_json::{Map, Value as Json};

use super::email::is_email;
use crate::config::RosterConfig;
use crate::port_types::{Statement, Value};
use crate::{Result, Services};

/// One kind of missing data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Missing {
    /// No email: the Student cannot start onboarding.
    NoEmail,
    /// An email that is not a valid address.
    InvalidEmail,
    /// The attribute `key` has no value.
    Attribute(String),
}

/// What is missing from a Student with this `email` and `attributes`.
pub(crate) fn of(
    email: Option<&str>,
    attributes: &Map<String, Json>,
    roster: &RosterConfig,
) -> Vec<Missing> {
    let mut out = Vec::new();
    match email.map(str::trim).filter(|e| !e.is_empty()) {
        None => out.push(Missing::NoEmail),
        Some(e) if !is_email(e) => out.push(Missing::InvalidEmail),
        Some(_) => {}
    }
    for (key, _) in roster.attributes_in_order() {
        if no_value(attributes.get(key)) {
            out.push(Missing::Attribute(key.to_string()));
        }
    }
    out
}

fn no_value(v: Option<&Json>) -> bool {
    match v {
        None | Some(Json::Null) => true,
        Some(Json::String(s)) => s.trim().is_empty(),
        Some(_) => false,
    }
}

/// The ids of the Students whose email is not a valid address (a JSON
/// array, for `json_each`).
async fn invalid_email_ids(s: &Services) -> Result<String> {
    let rows =
        s.db.query(Statement::new(
            "SELECT id, email FROM student WHERE email IS NOT NULL AND trim(email) <> ''",
        ))
        .await?;
    let mut ids = Vec::new();
    for i in 0..rows.len() {
        let email = rows.get(i, "email").and_then(Value::as_text).unwrap_or("");
        if let (false, Some(id)) = (
            is_email(email.trim()),
            rows.get(i, "id").and_then(Value::as_text),
        ) {
            ids.push(id.to_string());
        }
    }
    Ok(Json::from(ids).to_string())
}

/// The `WHERE` condition for Students with missing data and its bound
/// values (in order): one for the invalid ids, then one per attribute.
pub(super) async fn condition(s: &Services, roster: &RosterConfig) -> Result<(String, Vec<Value>)> {
    let mut sql = vec![
        "email IS NULL".to_string(),
        "trim(email) = ''".into(),
        "id IN (SELECT value FROM json_each(?))".into(),
    ];
    let mut binds = vec![Value::from(invalid_email_ids(s).await?)];
    for (key, _) in roster.attributes_in_order() {
        // Keys are `[a-z][a-z0-9_]*` (`RosterConfig::check`), quoted anyway.
        sql.push("COALESCE(trim(json_extract(attributes, ?)), '') = ''".into());
        binds.push(Value::from(format!("$.\"{key}\"")));
    }
    Ok((format!("({})", sql.join(" OR ")), binds))
}
