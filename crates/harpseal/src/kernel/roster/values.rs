//! The distinct values of a roster attribute, with how many Students hold
//! each (#164): for a Module's filter or report, so it never reads
//! `student.attributes` JSON itself.
//!
//! Values are compared and returned as the Audience compares them: the
//! stored JSON value as text (`2`, `3.5`, `MSC`). A Student without the
//! attribute (or with JSON `null`) is not counted. Only `active` Students
//! count unless [`ValuesScope::include_left`]. No Permission and no audit:
//! counts per value name nobody.

use std::collections::BTreeMap;

use crate::audience::MAX_VALUES;
use crate::config::RosterConfig;
use crate::port_types::{Statement, Value};
use crate::{Deployment, Error, Result};

/// Which Students [`attribute_values`] counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ValuesScope {
    /// Also count `left` Students (default: `active` only).
    pub include_left: bool,
    /// Other attributes the Students must hold: key → any of these values
    /// (compared as text), AND-ed over keys, as an Audience's attribute
    /// filter. At most 50 values in all.
    pub attributes: BTreeMap<String, Vec<String>>,
}

impl ValuesScope {
    /// `active` Students, no other filter.
    pub fn active() -> Self {
        Self::default()
    }

    /// Only Students whose `key` is one of `values`.
    pub fn with(mut self, key: impl Into<String>, values: &[&str]) -> Self {
        self.attributes
            .entry(key.into())
            .or_default()
            .extend(values.iter().map(|v| v.to_string()));
        self
    }
}

/// One value of an attribute and its Students.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AttributeValue {
    /// The stored value as text.
    pub value: String,
    /// Students holding it (within the scope).
    pub students: u64,
}

/// The distinct values of `[roster.attributes.<key>]` among the Students
/// in `scope`, with their counts: numbers in numeric order for an
/// `integer` or `number` attribute, else by text. An attribute (here or in
/// the scope) that `[roster]` does not declare, an empty scope value or
/// more than 50 scope values is [`Error::Invalid`].
pub async fn attribute_values(
    d: &Deployment,
    key: &str,
    scope: &ValuesScope,
) -> Result<Vec<AttributeValue>> {
    let roster = &d.config().roster;
    let st = statement(roster, key, scope)?;
    let rows = d.services().db.query(st).await?;
    let mut out: Vec<AttributeValue> = (0..rows.len())
        .filter_map(|i| {
            let value = rows.get(i, "value").and_then(Value::as_text)?.to_string();
            let n = rows.get(i, "n").and_then(Value::as_i64)?;
            Some(AttributeValue {
                value,
                students: u64::try_from(n).unwrap_or(0),
            })
        })
        .collect();
    let numeric = roster
        .attributes
        .get(key)
        .is_some_and(|s| s.kind.is_numeric());
    if numeric {
        let num = |v: &AttributeValue| v.value.parse::<f64>().ok();
        out.sort_by(|a, b| match (num(a), num(b)) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.value.cmp(&b.value),
        });
    } else {
        out.sort_by(|a, b| a.value.cmp(&b.value));
    }
    Ok(out)
}

fn path(key: &str) -> String {
    format!("$.\"{key}\"")
}

/// The query: values bound, never spliced; keys checked against `[roster]`
/// first and bound as JSON paths.
fn statement(roster: &RosterConfig, key: &str, scope: &ValuesScope) -> Result<Statement> {
    let unknown = |k: &str| Error::Invalid(format!("unknown attribute `{k}`"));
    if !roster.attributes.contains_key(key) {
        return Err(unknown(key));
    }
    let mut params = vec![path(key), path(key)];
    let mut sql = String::from(
        "SELECT CAST(json_extract(s.attributes, ?) AS TEXT) AS value, count(*) AS n \
         FROM student s WHERE json_type(s.attributes, ?) NOT IN ('null', 'object', 'array')",
    );
    if !scope.include_left {
        sql.push_str(" AND s.status = 'active'");
    }
    let total: usize = scope.attributes.values().map(Vec::len).sum();
    if total > MAX_VALUES {
        return Err(Error::Invalid(format!("more than {MAX_VALUES} values")));
    }
    for (k, values) in &scope.attributes {
        if !roster.attributes.contains_key(k) {
            return Err(unknown(k));
        }
        if values.is_empty() || values.iter().any(|v| v.trim().is_empty()) {
            return Err(Error::Invalid(format!("empty value for `{k}`")));
        }
        let marks = vec!["?"; values.len()].join(", ");
        sql.push_str(&format!(
            " AND CAST(json_extract(s.attributes, ?) AS TEXT) IN ({marks})"
        ));
        params.push(path(k));
        params.extend(values.iter().map(|v| v.trim().to_string()));
    }
    sql.push_str(" GROUP BY value");
    Ok(params
        .into_iter()
        .fold(Statement::new(sql), |s, p| s.bind(p)))
}
