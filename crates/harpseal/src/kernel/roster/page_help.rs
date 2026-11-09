//! The roster's upload help (#127): the columns `[roster]` maps, with what
//! each holds, whether it may be empty and an example, the file rules and
//! the template, beside the file input and again under a refused header.

use maud::Markup;

use super::view::{BASE, View};
use crate::config::{AttributeSpec, AttributeType, RosterConfig};
use crate::ui::HelpColumn;

/// An example cell of attribute `a`: its first allowed value, the bottom
/// (else top) of its range, or a typical value of its type; none for text.
fn example(a: &AttributeSpec) -> String {
    match a.kind {
        AttributeType::Enum => a.values.first().cloned().unwrap_or_default(),
        AttributeType::Integer | AttributeType::Number => match a.min.as_ref().or(a.max.as_ref()) {
            Some(n) => n.to_string(),
            None if a.kind == AttributeType::Integer => "1".into(),
            None => "2.50".into(),
        },
        AttributeType::Date => "2026-06-15".into(),
        _ => String::new(),
    }
}

/// The columns, in template order (see `file::expected_headers`).
pub(super) fn columns(v: &View<'_>, roster: &RosterConfig) -> Vec<HelpColumn> {
    let col = |header: &str, key: &str, required: bool, example: String| HelpColumn {
        header: header.to_string(),
        holds: v.t(key),
        required,
        example,
    };
    let mut rows = vec![
        col(
            &roster.student_code,
            "kernel.students.code",
            true,
            v.t("kernel.upload.example_code"),
        ),
        col(
            &roster.display_name,
            "kernel.students.name",
            true,
            v.t("kernel.upload.example_name"),
        ),
        col(
            &roster.email,
            "kernel.students.email",
            false,
            v.t("kernel.upload.example_email"),
        ),
    ];
    if let Some(l) = &roster.locale {
        let tag = v.s.config.default_locale.0.clone();
        rows.push(col(&l.column, "kernel.students.locale", false, tag));
    }
    if let Some(t) = &roster.tags {
        let mut c = col(&t.column, "kernel.students.tags", false, String::new());
        c.holds = format!(
            "{} ({} \"{}\")",
            c.holds,
            v.t("kernel.roster.separator"),
            t.separator
        );
        c.example = format!(
            "{}{}{}",
            v.t("kernel.upload.example_tag"),
            t.separator,
            v.t("kernel.upload.example_tag2")
        );
        rows.push(c);
    }
    let default = &v.s.config.default_locale;
    for (key, a) in roster.attributes_in_order() {
        let mut holds = a.label_in(key, &v.staff.locale, default).to_string();
        if a.kind == AttributeType::Enum {
            // The default Locale's labels: the ones a cell may hold.
            let all: Vec<String> = a
                .values
                .iter()
                .map(|x| a.labelled(x, default, default))
                .collect();
            holds = format!(
                "{holds} ({} {})",
                v.t("kernel.roster.one_of"),
                all.join(", ")
            );
        }
        if let Some(range) = a.range_text() {
            holds = format!("{holds} ({range})");
        }
        rows.push(HelpColumn {
            header: a.column.clone(),
            holds,
            required: !a.optional,
            example: example(a),
        });
    }
    rows
}

/// The panel's body: rules, columns and, with `template`, its link.
pub(super) fn panel(v: &View<'_>, template: bool) -> Markup {
    let cols = columns(v, &v.s.config.roster);
    let href = format!("{BASE}/template.csv");
    let template = template.then_some(href.as_str());
    crate::ui::upload_panel(v.s, v.staff, &cols, template, true)
}
