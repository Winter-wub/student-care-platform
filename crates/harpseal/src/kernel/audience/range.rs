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
//! - A bound is read as a roster number is ([`crate::sheet::number`]):
//!   `12,500` and `12500` are the same; the number, not the text, is bound
//!   in the query.

use serde::{Deserialize, Serialize};

use crate::config::RosterConfig;
use crate::sheet::{Blank, number};
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

    /// The bounds as numbers ([`crate::sheet::number`]), checked: at
    /// least one, each finite, the lower not above the upper.
    pub fn bounds(&self) -> std::result::Result<(Option<f64>, Option<f64>), RangeError> {
        let num = |b: &Option<String>| match b {
            None => Ok(None),
            Some(s) => number(s, Blank::Missing).map_err(|_| RangeError::NotNumber),
        };
        let (min, max) = (num(&self.min)?, num(&self.max)?);
        if min.is_none() && max.is_none() {
            return Err(RangeError::Empty);
        }
        if let (Some(lo), Some(hi)) = (min, max)
            && lo > hi
        {
            return Err(RangeError::Reversed);
        }
        Ok((min, max))
    }
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
            Some(spec) if !spec.kind.is_numeric() => {
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
/// real) within each given bound. `range` passed [`check`]; an unusable
/// one selects nobody.
pub(super) fn push_sql(key: &str, range: &Range, sql: &mut String, params: &mut Vec<String>) {
    let path = format!("$.\"{key}\"");
    sql.push_str(" AND json_type(s.attributes, ?) IN ('integer', 'real')");
    params.push(path.clone());
    let Ok((min, max)) = range.bounds() else {
        sql.push_str(" AND 0");
        return;
    };
    for (bound, op) in [(min, ">="), (max, "<=")] {
        if let Some(b) = bound {
            sql.push_str(&format!(
                " AND CAST(json_extract(s.attributes, ?) AS REAL) {op} CAST(? AS REAL)"
            ));
            params.push(path.clone());
            // `Display` of an `f64` never uses an exponent.
            params.push(b.to_string());
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
        assert_eq!(Range::new("1e3", "").bounds(), Err(RangeError::NotNumber));
        assert_eq!(
            Range::new("1,000", "12,500.5").bounds(),
            Ok((Some(1000.0), Some(12500.5)))
        );
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
