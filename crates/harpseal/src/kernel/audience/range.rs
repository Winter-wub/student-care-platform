//! Range rules of an [`Audience`](super::Audience) (#156): a numeric
//! attribute (`integer` or `number` in `[roster.attributes]`, such as a
//! GPA) between two bounds instead of a list of exact values.
//!
//! - Both bounds are inclusive; either may be left out, never both (a
//!   range with neither bound is no rule and is never stored).
//! - A Student matches only when the stored value is a JSON number within
//!   the bounds: a missing or non-numeric value never matches, so a range
//!   never widens an Audience.
//! - Ranges are AND-ed with every other filter, as attributes are.

use serde::{Deserialize, Serialize};

use crate::config::{AttributeType, RosterConfig};
use crate::{Error, Result};

/// An inclusive range over one numeric attribute. Bounds are kept as typed
/// (trimmed), so a stored rule reads back as Staff wrote it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Range {
    /// Smallest value, inclusive. `None`: no lower bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    /// Largest value, inclusive. `None`: no upper bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
}

/// Why a [`Range`] cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RangeError {
    /// Neither bound is given.
    Empty,
    /// A bound is not a finite number.
    NotNumber,
    /// The lower bound is above the upper one.
    Reversed,
}

impl Range {
    /// A range from two typed bounds, each trimmed; an empty one is no
    /// bound.
    pub fn new(min: &str, max: &str) -> Self {
        let bound = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
        Self {
            min: bound(min),
            max: bound(max),
        }
    }

    /// Neither bound given.
    pub fn is_empty(&self) -> bool {
        self.min.is_none() && self.max.is_none()
    }

    /// The bounds as numbers, checked: at least one, each finite, the
    /// lower not above the upper.
    pub fn bounds(&self) -> std::result::Result<(Option<f64>, Option<f64>), RangeError> {
        if self.is_empty() {
            return Err(RangeError::Empty);
        }
        let num = |b: &Option<String>| match b {
            None => Ok(None),
            Some(s) => match s.trim().parse::<f64>() {
                Ok(n) if n.is_finite() => Ok(Some(n)),
                _ => Err(RangeError::NotNumber),
            },
        };
        let (min, max) = (num(&self.min)?, num(&self.max)?);
        if let (Some(lo), Some(hi)) = (min, max)
            && lo > hi
        {
            return Err(RangeError::Reversed);
        }
        Ok((min, max))
    }
}

/// Whether `kind` takes a range: `integer` and `number`.
pub fn numeric(kind: AttributeType) -> bool {
    matches!(kind, AttributeType::Integer | AttributeType::Number)
}

/// Checks each range against the roster schema: a known numeric attribute
/// and usable bounds. `Error::Invalid` names the key, never a value.
pub(super) fn check<'a>(
    ranges: impl IntoIterator<Item = (&'a String, &'a Range)>,
    roster: &RosterConfig,
) -> Result<()> {
    for (key, range) in ranges {
        match roster.attributes.get(key) {
            None => return Err(Error::Invalid(format!("unknown attribute `{key}`"))),
            Some(spec) if !numeric(spec.kind) => {
                return Err(Error::Invalid(format!("`{key}` is not a number")));
            }
            Some(_) => {}
        }
        if range.bounds().is_err() {
            return Err(Error::Invalid(format!("bad range for `{key}`")));
        }
    }
    Ok(())
}

/// Appends `range`'s condition on `key` to `sql` and its binds to
/// `params`: the value must be a JSON number (`json_type` integer or
/// real) within each given bound.
pub(super) fn push_sql(key: &str, range: &Range, sql: &mut String, params: &mut Vec<String>) {
    let path = format!("$.\"{key}\"");
    sql.push_str(" AND json_type(s.attributes, ?) IN ('integer', 'real')");
    params.push(path.clone());
    for (bound, op) in [(&range.min, ">="), (&range.max, "<=")] {
        if let Some(b) = bound {
            sql.push_str(&format!(
                " AND CAST(json_extract(s.attributes, ?) AS REAL) {op} CAST(? AS REAL)"
            ));
            params.push(path.clone());
            params.push(b.trim().to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_are_checked() {
        assert_eq!(Range::new(" ", "").bounds(), Err(RangeError::Empty));
        assert_eq!(Range::new("3", "x").bounds(), Err(RangeError::NotNumber));
        assert_eq!(Range::new("inf", "").bounds(), Err(RangeError::NotNumber));
        assert_eq!(Range::new("3.5", "3").bounds(), Err(RangeError::Reversed));
        assert_eq!(Range::new("3", "3").bounds(), Ok((Some(3.0), Some(3.0))));
        assert_eq!(Range::new("", " 2.5 ").bounds(), Ok((None, Some(2.5))));
        assert_eq!(Range::new(" 2 ", "").min.as_deref(), Some("2"));
    }

    #[test]
    fn a_bound_left_out_is_not_serialized() {
        let json = serde_json::to_string(&Range::new("3", "")).unwrap();
        assert_eq!(json, r#"{"min":"3"}"#);
        let back: Range = serde_json::from_str(r#"{"max":"2"}"#).unwrap();
        assert_eq!(back, Range::new("", "2"));
    }
}
