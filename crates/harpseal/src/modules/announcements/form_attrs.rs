//! The Audience's attribute filters (#129; typed by #156), each drawn by
//! what `[roster]` knows of it:
//!
//! - an `enum`: a checkbox per value;
//! - an `integer` with `min` and `max` at most [`MAX_CHECKS`] apart (a
//!   year): a checkbox per whole number; a value saved outside them stays
//!   as one more checked box, so it is never dropped;
//! - another `integer` or a `number` (a GPA): "ตั้งแต่" and "ถึง" boxes
//!   (`attr_min.<key>`, `attr_max.<key>`, a [`Range`]); exact values saved
//!   before #156 stay in a typed box under them;
//! - `text` and `date`: one typed box, any of the values separated by
//!   commas (`attr.<key>`), as before.
//!
//! Every attribute is labelled: a group of controls is a fieldset with a
//! legend.

use maud::{Markup, html};

use super::form::Fields;
use super::view::View;
use crate::audience::{Range, numeric};
use crate::config::{AttributeSpec, AttributeType};
use crate::kernel::text::fill;

/// Most checkboxes an `integer` attribute gets before it is a range.
pub(super) const MAX_CHECKS: i64 = 12;

/// How one attribute is filtered on the form.
enum Control {
    /// A checkbox per value (value, label); `true`: short ones, in a
    /// wrapping row.
    Checks(Vec<(String, String)>, bool),
    /// "ตั้งแต่" and "ถึง".
    Range,
    /// One typed box.
    Typed,
}

/// The whole numbers from `min` to `max` of a small `integer` attribute.
fn whole_numbers(spec: &AttributeSpec) -> Option<Vec<String>> {
    let lo = spec.min.as_ref()?.as_i64()?;
    let hi = spec.max.as_ref()?.as_i64()?;
    (lo <= hi && hi - lo < MAX_CHECKS).then(|| (lo..=hi).map(|n| n.to_string()).collect())
}

fn control(v: &View<'_>, spec: &AttributeSpec, chosen: &[String]) -> Control {
    let (locale, default) = (&v.staff.locale, &v.s.config.default_locale);
    if spec.kind == AttributeType::Enum {
        let label = |x: &String| spec.value_label(x, locale, default).to_string();
        let values = spec.values.iter().map(|x| (x.clone(), label(x))).collect();
        return Control::Checks(values, false);
    }
    if spec.kind == AttributeType::Integer
        && let Some(mut values) = whole_numbers(spec)
    {
        // A value saved outside `min`..`max` stays, checked.
        for c in chosen {
            if !values.contains(c) {
                values.push(c.clone());
            }
        }
        return Control::Checks(values.into_iter().map(|x| (x.clone(), x)).collect(), true);
    }
    if numeric(spec.kind) {
        Control::Range
    } else {
        Control::Typed
    }
}

/// The attribute filters of `f`, in `[roster]` order.
pub(super) fn attributes(v: &View<'_>, f: &Fields) -> Markup {
    let roster = &v.s.config.roster;
    html! {
        @for (key, spec) in roster.attributes_in_order() {
            @let chosen = f.attrs.get(key).cloned().unwrap_or_default();
            @let label = spec.label_in(key, &v.staff.locale, &v.s.config.default_locale);
            @match control(v, spec, &chosen) {
                Control::Checks(values, short) => {
                    fieldset class="checks" {
                        legend { (label) }
                        div class=[short.then_some("check-row")] {
                            @for (value, shown) in &values {
                                label class="check" {
                                    input type="checkbox" name=(format!("attr.{key}")) value=(value)
                                        checked[chosen.contains(value)];
                                    " " (shown)
                                }
                            }
                        }
                    }
                }
                Control::Range => (range(v, key, label, f, &chosen, spec.kind)),
                Control::Typed => (typed(v, key, label, &chosen, "announcements.form.any_of")),
            }
        }
    }
}

/// One typed box `attr.<key>`.
fn typed(v: &View<'_>, key: &str, label: &str, chosen: &[String], hint: &str) -> Markup {
    html! {
        div class="field" {
            label for=(format!("attr-{key}")) { (label) }
            input id=(format!("attr-{key}")) name=(format!("attr.{key}")) value=(chosen.join(", "))
                placeholder=(v.t(hint));
        }
    }
}

/// "ตั้งแต่" and "ถึง" of a numeric attribute, and, when the form already
/// holds exact values for it, those in a typed box below.
fn range(
    v: &View<'_>,
    key: &str,
    label: &str,
    f: &Fields,
    chosen: &[String],
    kind: AttributeType,
) -> Markup {
    let (lo, hi) = f.ranges.get(key).cloned().unwrap_or_default();
    let mode = if kind == AttributeType::Integer {
        "numeric"
    } else {
        "decimal"
    };
    let help = format!("range-help-{key}");
    let bound = |side: &str, value: &str, text: &str| {
        let id = format!("attr-{side}-{key}");
        html! {
            div class="field" {
                label for=(id) { (v.t(text)) }
                input id=(id) name=(format!("attr_{side}.{key}")) value=(value) inputmode=(mode)
                    autocomplete="off" aria-describedby=(help);
            }
        }
    };
    html! {
        fieldset class="range" {
            legend { (label) }
            div class="row2" {
                (bound("min", &lo, "announcements.form.range_min"))
                (bound("max", &hi, "announcements.form.range_max"))
            }
            p id=(help) class="muted" { (v.t("announcements.form.range_help")) }
            @if !chosen.is_empty() {
                (typed(v, key, &v.t("announcements.form.exact_values"), chosen, "announcements.form.any_of"))
            }
        }
    }
}

/// `r` as Staff read it: "ตั้งแต่ 3 ถึง 4", "ตั้งแต่ 3 ขึ้นไป", "ไม่เกิน 2".
pub(crate) fn range_text(v: &View<'_>, r: &Range) -> String {
    match (&r.min, &r.max) {
        (Some(lo), Some(hi)) => fill(
            &v.t("announcements.form.range_between"),
            &[("min", lo), ("max", hi)],
        ),
        (Some(lo), None) => fill(&v.t("announcements.form.range_from"), &[("min", lo)]),
        (None, Some(hi)) => fill(&v.t("announcements.form.range_to"), &[("max", hi)]),
        (None, None) => String::new(),
    }
}
