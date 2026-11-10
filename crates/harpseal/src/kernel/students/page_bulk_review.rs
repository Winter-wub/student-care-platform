//! The pages of a bulk action (#145) besides the list: the review step and
//! a refusal. The review names the Students who will change and posts
//! their set's token ([`super::bulk::set_token`]), so a confirm acts on
//! exactly the Students Staff saw.

use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};

use super::bulk::Selection;
use super::page_bulk::{Action, PATH, TYPED};
use super::view::View;

/// Student codes the review lists before "and n more".
const SHOWN_CODES: usize = 20;

/// One review step.
pub(super) struct Review<'a> {
    pub(super) sel: &'a Selection,
    pub(super) action: Action,
    pub(super) tag: &'a str,
    /// Every Student selected (a page posts them again as ticked rows).
    pub(super) selected: &'a [String],
    /// The codes of the Students the action changes, in list order.
    pub(super) codes: &'a [String],
    /// The token of the set of Students the action changes.
    pub(super) token: &'a str,
    /// The Students or their count differ from the ones confirmed.
    pub(super) changed: bool,
    /// The typed number did not match.
    pub(super) wrong: bool,
}

impl Review<'_> {
    fn reach(&self) -> usize {
        self.codes.len()
    }
}

pub(super) fn review(v: &View<'_>, r: &Review<'_>) -> Response {
    let h1 = v.t("kernel.students.bulk_review");
    let title = if r.wrong {
        v.t("kernel.students.bulk_fix_title")
    } else {
        h1.clone()
    };
    let a = r.action.as_str();
    let reach = r.reach();
    let n = crate::ui::number(reach);
    let fill = |key: &str, n: &str| v.t(key).replace("{n}", n).replace("{tag}", r.tag);
    let skipped = r.selected.len().saturating_sub(reach);
    let typed = r.action == Action::Unlink && reach > 1;
    let more = reach.saturating_sub(SHOWN_CODES);
    let status = if r.wrong {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::OK
    };
    let body = html! {
        div class="card narrow bulk-review" {
            h1 { (h1) }
            // One message per update: the change, or the error summary.
            @if r.changed {
                p class="blocked-reason" role="status" { (v.t("kernel.students.bulk_changed")) }
            } @else if r.wrong {
                p class="flash err" role="alert" { (v.t("kernel.students.bulk_fix")) }
            }
            p class="reach" id="bulk-reach" { strong { (fill(&format!("kernel.students.bulk_review_{a}"), &n)) } }
            @if skipped > 0 {
                p class="muted" { (fill(&format!("kernel.students.bulk_skip_{a}"), &crate::ui::number(skipped))) }
            }
            @if reach > 0 {
                h2 class="codes-title" { (v.t("kernel.students.bulk_codes")) }
                p class="codes" {
                    (r.codes.iter().take(SHOWN_CODES).cloned().collect::<Vec<_>>().join(", "))
                    @if more > 0 { " " (v.t("kernel.students.bulk_more").replace("{n}", &crate::ui::number(more))) }
                }
            }
            form id="bulk-confirm" method="post" action=(PATH) {
                (v.staff.csrf_field())
                (r.sel.hidden(r.selected))
                input type="hidden" name="action" value=(a);
                input type="hidden" name="tag" value=(r.tag);
                input type="hidden" name="n" value=(reach);
                input type="hidden" name="set" value=(r.token);
                input type="hidden" name="confirm" value="yes";
                @if typed { (typed_field(v, &n, r.wrong && !r.changed)) }
                div class="actions" {
                    @if reach == 0 {
                        (crate::ui::blocked_button(
                            &fill(&format!("kernel.students.bulk_confirm_{a}"), "0"),
                            &v.t(&format!("kernel.students.bulk_nothing_{a}")),
                            "bulk-blocked",
                        ))
                    } @else if r.action == Action::Unlink {
                        (crate::ui::danger_button(&fill("kernel.students.bulk_confirm_unlink", &n)))
                    } @else {
                        button type="submit" { (fill(&format!("kernel.students.bulk_confirm_{a}"), &n)) }
                    }
                    a class="btn secondary" href=(r.sel.list_href()) { (v.t("kernel.students.cancel")) }
                }
            }
        }
    };
    v.page(status, &title, body)
}

/// The typed number an unlink of more than one Student needs (#135).
fn typed_field(v: &View<'_>, n: &str, wrong: bool) -> Markup {
    let described = if wrong {
        "bulk-reach bulk-typed-error"
    } else {
        "bulk-reach"
    };
    html! {
        div class="field confirm-reach" {
            label for="bulk-typed" { (v.t("kernel.students.bulk_typed").replace("{n}", n)) }
            input id="bulk-typed" type="text" name=(TYPED) required inputmode="numeric"
                autocomplete="off" spellcheck="false" enterkeyhint="done"
                aria-invalid=[wrong.then_some("true")] aria-describedby=(described);
            @if wrong {
                p id="bulk-typed-error" class="field-error" {
                    (v.t("kernel.students.bulk_typed_wrong").replace("{n}", n))
                }
            }
        }
    }
}

/// A refusal page: the problem with the count, what to do, and the way
/// back.
pub(super) fn refused(v: &View<'_>, key: &str, n: usize, back: &str) -> Response {
    let title = v.t("kernel.students.bulk_refused");
    let text = v.t(key).replace("{n}", &crate::ui::number(n));
    let body = html! {
        div class="card narrow" {
            h1 { (title) }
            p class="blocked-reason" role="status" { (text) }
            div class="actions" {
                a class="btn secondary" href=(back) { (v.t("kernel.students.bulk_back")) }
            }
        }
    };
    v.page(StatusCode::UNPROCESSABLE_ENTITY, &title, body)
}
