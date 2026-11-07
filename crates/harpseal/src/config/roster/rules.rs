//! What a roster attribute may hold and how Staff see it (#81): an
//! inclusive `min` / `max` on `integer` and `number`, and a display label
//! per Locale for each value of an `enum`.
//!
//! ```toml
//! [roster.attributes.year]
//! type = "integer"
//! min = 1
//! max = 6
//!
//! [roster.attributes.status]
//! type = "enum"
//! values = ["current", "graduated"]
//! value_labels = { current = { th = "กำลังศึกษา", en = "Studying" } }
//! ```
//!
//! A label is only shown: the stored value, the Audience filters and the
//! Mini App keep the value. The roster import accepts the value or the
//! label in the Deployment's default Locale (see
//! [`AttributeSpec::parse_import`]).

use std::cmp::Ordering;
use std::collections::BTreeSet;

use serde_json::{Number, Value as Json};

use super::{AttrError, AttributeSpec, AttributeType};
use crate::message::Locale;

impl AttributeSpec {
    /// Whether a parsed `integer` / `number` value is within `min` / `max`.
    pub(super) fn in_range(&self, v: &Json) -> bool {
        let Json::Number(n) = v else { return true };
        let below = self.min.as_ref().is_some_and(|m| compare(n, m).is_lt());
        let above = self.max.as_ref().is_some_and(|m| compare(n, m).is_gt());
        !(below || above)
    }

    /// The allowed range as text (`1–6`, `≥ 1`, `≤ 6`), if any.
    pub fn range_text(&self) -> Option<String> {
        match (&self.min, &self.max) {
            (None, None) => None,
            (Some(a), Some(b)) => Some(format!("{a}–{b}")),
            (Some(a), None) => Some(format!("≥ {a}")),
            (None, Some(b)) => Some(format!("≤ {b}")),
        }
    }

    /// The label of the enum value `value` in `locale`, else in
    /// `fallback`, else the value itself.
    pub fn value_label<'a>(
        &'a self,
        value: &'a str,
        locale: &Locale,
        fallback: &Locale,
    ) -> &'a str {
        self.value_labels
            .get(value)
            .and_then(|l| l.get(&locale.0).or_else(|| l.get(&fallback.0)))
            .map_or(value, String::as_str)
    }

    /// A stored value as Staff read it: the label for an `enum` value,
    /// else [`AttributeSpec::display`].
    pub fn shown(&self, value: Option<&Json>, locale: &Locale, fallback: &Locale) -> String {
        let raw = Self::display(value);
        match self.kind {
            AttributeType::Enum if !raw.is_empty() => {
                self.value_label(&raw, locale, fallback).to_string()
            }
            _ => raw,
        }
    }

    /// The enum value `value` as `label (value)`; just the value when it
    /// has no label to show. For lists of what a cell may hold.
    pub fn labelled(&self, value: &str, locale: &Locale, fallback: &Locale) -> String {
        match self.value_label(value, locale, fallback) {
            l if l == value => l.to_string(),
            l => format!("{l} ({value})"),
        }
    }

    /// [`AttributeSpec::parse`] for a roster cell: an `enum` cell may also
    /// name a value by its label in `default_locale` (exact after trimming,
    /// ASCII letters in either case); the value to store is the value.
    /// A cell that is a value is never read as a label.
    pub fn parse_import(
        &self,
        raw: &str,
        default_locale: &Locale,
    ) -> Result<Option<Json>, AttrError> {
        let text = raw.trim();
        let by_label = (self.kind == AttributeType::Enum && !self.values.iter().any(|v| v == text))
            .then(|| self.value_named(text, default_locale))
            .flatten();
        self.parse(by_label.unwrap_or(raw))
    }

    fn value_named(&self, text: &str, locale: &Locale) -> Option<&str> {
        self.values
            .iter()
            .find(|v| {
                let label = self
                    .value_labels
                    .get(v.as_str())
                    .and_then(|l| l.get(&locale.0));
                label.is_some_and(|l| l.trim().eq_ignore_ascii_case(text))
            })
            .map(String::as_str)
    }
}

/// Compares two JSON numbers: exactly when both are whole numbers within
/// `i64`, else as `f64`.
fn compare(a: &Number, b: &Number) -> Ordering {
    match (a.as_i64(), b.as_i64()) {
        (Some(x), Some(y)) => x.cmp(&y),
        _ => {
            let (x, y) = (a.as_f64().unwrap_or(0.0), b.as_f64().unwrap_or(0.0));
            x.total_cmp(&y)
        }
    }
}

/// Refuses `min` / `max` on a type that is not a number, a `min` above
/// `max`, a fractional bound on an `integer`, labels on a type that is
/// not an `enum`, a label for a value that is not listed, an empty label,
/// and a label in a Locale that is not one of `known`. Then, for each
/// Locale that has a label (and the default Locale), what each value
/// *shows* there (its label, else the default Locale's, else the value)
/// must differ between values, or Staff could not tell them apart. In the
/// default Locale (the one the import reads) a label must also differ from
/// the other values, or a cell could name two values. The `Err` is the
/// problem.
pub(super) fn check(
    a: &AttributeSpec,
    default_locale: &Locale,
    known: &BTreeSet<String>,
) -> Result<(), String> {
    let numeric = matches!(a.kind, AttributeType::Integer | AttributeType::Number);
    for (name, bound) in [("min", &a.min), ("max", &a.max)] {
        let Some(n) = bound else { continue };
        if !numeric {
            return Err(format!("{name} only fits an integer or a number"));
        }
        if a.kind == AttributeType::Integer && n.as_i64().is_none() {
            return Err(format!("{name} {n} must be a whole number"));
        }
    }
    if let (Some(lo), Some(hi)) = (&a.min, &a.max)
        && compare(lo, hi).is_gt()
    {
        return Err(format!("min {lo} is above max {hi}"));
    }
    if a.value_labels.is_empty() {
        return Ok(());
    }
    if a.kind != AttributeType::Enum {
        return Err("value_labels only fit an enum".into());
    }
    let mut locales: BTreeSet<&str> = BTreeSet::from([default_locale.0.as_str()]);
    for (value, labels) in &a.value_labels {
        if !a.values.contains(value) {
            return Err(format!(
                "value_labels names `{value}`, which is not in values"
            ));
        }
        for (locale, label) in labels {
            if !known.contains(locale) {
                let all = known.iter().cloned().collect::<Vec<_>>().join("`, `");
                return Err(format!(
                    "the {locale} label of `{value}` names a Locale that is not configured \
                     (one of `{all}`)"
                ));
            }
            if label.trim().is_empty() {
                return Err(format!("the {locale} label of `{value}` is empty"));
            }
            locales.insert(locale);
        }
    }
    for locale in locales {
        let loc = Locale::new(locale);
        let shown: Vec<(&str, &str, bool)> = a
            .values
            .iter()
            .map(|v| {
                let labelled = a.value_label(v, &loc, default_locale);
                (v.as_str(), labelled.trim(), labelled != v.as_str())
            })
            .collect();
        for (i, (value, text, from_label)) in shown.iter().enumerate() {
            // Two bare values keep today's rules (`A` and `a` may coexist).
            let clash = shown[..i]
                .iter()
                .find(|(_, t, l)| (*l || *from_label) && t.eq_ignore_ascii_case(text));
            if let Some((other, _, _)) = clash {
                return Err(format!(
                    "`{other}` and `{value}` both show `{text}` in {locale} \
                     (a label is missing, or two values share one)"
                ));
            }
            let is_default = locale == default_locale.0;
            let other = a
                .values
                .iter()
                .find(|v| *v != value && v.trim().eq_ignore_ascii_case(text));
            if let (true, true, Some(other)) = (is_default, *from_label, other) {
                return Err(format!(
                    "the {locale} label `{text}` of `{value}` is also the value `{other}`"
                ));
            }
        }
    }
    Ok(())
}
