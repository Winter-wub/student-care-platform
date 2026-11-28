//! An attribute whose requirement depends on another attribute: a year
//! level every current Student needs but an alumnus no longer has, a
//! cohort only an alumnus has.
//!
//! ```toml
//! [roster.attributes.status]
//! type = "enum"
//! values = ["current", "graduated"]
//!
//! # Required, except for a graduate.
//! [roster.attributes.year]
//! type = "integer"
//! optional_when = { status = ["graduated"] }
//!
//! # May be empty, except for a graduate.
//! [roster.attributes.cohort]
//! type = "integer"
//! optional = true
//! required_when = { status = ["graduated"] }
//! ```
//!
//! The roster import checks each row (a blank cell is refused only when
//! the row's other value makes it required) and the Staff edit form checks
//! it when either field changes. Nothing else reads the condition: the
//! Audience filters and the Mini App see the values as stored.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value as Json};

use super::{AttributeSpec, AttributeType};
use crate::message::Locale;

impl AttributeSpec {
    /// The other attribute and its values the requirement depends on
    /// (`optional_when` or `required_when`), if any.
    pub fn condition(&self) -> Option<(&str, &[String])> {
        self.optional_when
            .iter()
            .chain(&self.required_when)
            .next()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// Whether the value may be empty for some Student: `optional`, or
    /// `optional_when` set.
    pub fn may_be_empty(&self) -> bool {
        self.optional || !self.optional_when.is_empty()
    }

    /// Whether a Student whose attributes are `attrs` needs a value here:
    /// not `optional`, unless `optional_when` matches `attrs`; or
    /// `optional` and `required_when` matches `attrs`. A condition matches
    /// when the other attribute holds one of the listed values (an empty
    /// one matches none).
    pub fn required_for(&self, attrs: &Map<String, Json>) -> bool {
        if !self.optional_when.is_empty() {
            return !matches(&self.optional_when, attrs);
        }
        if !self.required_when.is_empty() {
            return matches(&self.required_when, attrs);
        }
        !self.optional
    }

    /// The condition as Staff read it, e.g. `สถานะ: ศิษย์เก่า, พักการศึกษา`
    /// (the other attribute's label, then its values' labels).
    pub fn condition_text(
        &self,
        roster: &super::RosterConfig,
        locale: &Locale,
        fallback: &Locale,
    ) -> Option<String> {
        let (key, values) = self.condition()?;
        let other = roster.attributes.get(key)?;
        let shown: Vec<&str> = values
            .iter()
            .map(|v| other.value_label(v, locale, fallback))
            .collect();
        Some(format!(
            "{}: {}",
            other.label_in(key, locale, fallback),
            shown.join(", ")
        ))
    }
}

fn matches(cond: &BTreeMap<String, Vec<String>>, attrs: &Map<String, Json>) -> bool {
    cond.iter().all(|(k, values)| match attrs.get(k) {
        Some(Json::String(s)) => values.iter().any(|v| v == s),
        _ => false,
    })
}

/// Refuses a condition that is not one entry, names an attribute that is
/// not an `enum` of `all` (or this one, or one with a condition of its
/// own), lists no value or a value that attribute does not have, and
/// `optional_when` with `optional = true` or with `required_when`, or
/// `required_when` without `optional = true`. The `Err` is the problem.
pub(super) fn check(
    key: &str,
    a: &AttributeSpec,
    all: &BTreeMap<String, AttributeSpec>,
) -> Result<(), String> {
    let (when, name) = match (a.optional_when.is_empty(), a.required_when.is_empty()) {
        (true, true) => return Ok(()),
        (false, false) => return Err("optional_when and required_when cannot both be set".into()),
        (false, true) if a.optional => {
            return Err("optional_when fits a required attribute, not `optional = true`".into());
        }
        (true, false) if !a.optional => {
            return Err("required_when needs `optional = true`".into());
        }
        (false, true) => (&a.optional_when, "optional_when"),
        (true, false) => (&a.required_when, "required_when"),
    };
    let [(other, values)] = when.iter().collect::<Vec<_>>()[..] else {
        return Err(format!("{name} names one attribute"));
    };
    if other == key {
        return Err(format!("{name} cannot name the attribute itself"));
    }
    let Some(spec) = all.get(other) else {
        return Err(format!("{name}: no attribute `{other}`"));
    };
    if spec.kind != AttributeType::Enum {
        return Err(format!("{name}: `{other}` must be an enum"));
    }
    if spec.condition().is_some() {
        return Err(format!("{name}: `{other}` has a condition of its own"));
    }
    if values.is_empty() {
        return Err(format!("{name}: list at least one value of `{other}`"));
    }
    let mut seen = BTreeSet::new();
    for v in values.iter() {
        if !spec.values.contains(v) {
            return Err(format!("{name}: `{other}` has no value `{v}`"));
        }
        if !seen.insert(v) {
            return Err(format!("{name}: value `{v}` is listed twice"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "condition_tests.rs"]
mod tests;
