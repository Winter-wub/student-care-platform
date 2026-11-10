//! The bulk form of the Students list (#145): a checkbox per row, the
//! choice of rows (ticked, this page, everything matching the search), the
//! count, and one button per action the Staff member's Permissions allow.
//! It works as a plain form; HTMX only refreshes the count.

use maud::{Markup, html};

use super::page_bulk::{Action, COUNT_PATH, count_line};
use super::view::View;

/// The actions `v`'s Staff member may take; none means no bulk form.
pub(super) fn allowed(v: &View<'_>) -> Vec<Action> {
    [
        Action::TagAdd,
        Action::TagRemove,
        Action::Unlink,
        Action::Announce,
    ]
    .into_iter()
    .filter(|a| a.allowed(v))
    .collect()
}

/// The form's hidden fields: the list's filter and page.
pub(super) fn hidden(v: &View<'_>, query: &str, missing: bool, page: i64) -> Markup {
    html! {
        (v.staff.csrf_field())
        input type="hidden" name="q" value=(query);
        @if missing { input type="hidden" name="missing" value="1"; }
        input type="hidden" name="page" value=(page);
    }
}

/// The bar above the table: which rows, the count, the Tag name and the
/// action buttons. `on_page`: rows on this page; `total`: rows matching.
pub(super) fn bar(v: &View<'_>, actions: &[Action], on_page: usize, total: i64) -> Markup {
    let n = |k: &str, n: String| v.t(k).replace("{n}", &n);
    let tags = actions
        .iter()
        .any(|a| matches!(a, Action::TagAdd | Action::TagRemove));
    let which = [
        ("picked", v.t("kernel.students.bulk_which_picked")),
        (
            "page",
            n(
                "kernel.students.bulk_which_page",
                crate::ui::number(on_page),
            ),
        ),
        (
            "filter",
            n(
                "kernel.students.bulk_which_filter",
                crate::ui::number(total),
            ),
        ),
    ];
    html! {
        section class="card bulk-bar" aria-labelledby="bulk-title" {
            h2 id="bulk-title" { (v.t("kernel.students.bulk_title")) }
            fieldset class="checks bulk-which" {
                legend { (v.t("kernel.students.bulk_which")) }
                @for (value, label) in &which {
                    label class="check" {
                        input type="radio" name="which" value=(value) checked[*value == "picked"];
                        " " (label)
                    }
                }
            }
            div hx-post=(COUNT_PATH) hx-trigger="change from:closest form"
                hx-include="closest form" hx-target="#bulk-count" hx-swap="innerHTML" {
                (count_line(v, 0))
            }
            @if tags {
                div class="field bulk-tag" {
                    label for="bulk-tag" { (v.t("kernel.students.bulk_tag")) }
                    input id="bulk-tag" name="tag" maxlength="64" autocomplete="off";
                }
            }
            div class="actions bulk-actions" {
                @for a in actions {
                    button type="submit" class="secondary" name="action" value=(a.as_str()) {
                        (v.t(&format!("kernel.students.bulk_{}", a.as_str())))
                    }
                }
            }
        }
    }
}

/// The checkbox cell of a row.
pub(super) fn pick(v: &View<'_>, id: &str, name: &str) -> Markup {
    html! {
        td class="pick" data-label=(v.t("kernel.students.bulk_col")) {
            label class="check" {
                input type="checkbox" name="student" value=(id);
                span class="visually-hidden" { (v.t("kernel.students.bulk_pick").replace("{name}", name)) }
            }
        }
    }
}
