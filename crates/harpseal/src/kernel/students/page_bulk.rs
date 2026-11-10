//! `POST /admin/students/bulk` (#145): a bulk action on the list's
//! selection ([`super::bulk`]). `action` names it: `tag_add`, `tag_remove`
//! and `unlink` show a review step naming the count (and, for an unlink of
//! more than one Student, ask for that number typed, #120/#135), then act
//! on `confirm=yes`; `announce` opens the announcement editor with the
//! selection as a hand-picked Audience. Each action checks its own
//! Permission here, on top of the route's `kernel.student.view`.
//!
//! `POST /admin/students/bulk/count` is the HTMX count of the selection:
//! the text of `#bulk-count`.

use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use super::bulk::{self, Chosen, MAX_BULK, MAX_UNLINK, Selection, Which};
use super::bulk_write::{self as write, Unlinked};
use super::view::View;
use crate::kernel::LINK_REVOKE;
use crate::{Actor, AppState, CurrentStaff, RequestCtx};

/// The route of the bulk form.
pub(super) const PATH: &str = "/admin/students/bulk";
/// The route of its HTMX count.
pub(super) const COUNT_PATH: &str = "/admin/students/bulk/count";
/// The typed confirmation field.
const TYPED: &str = "typed";
/// Student codes the review lists before "and n more".
const SHOWN_CODES: usize = 20;

type Pairs = Form<Vec<(String, String)>>;

/// The bulk actions, with the Permission each needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    TagAdd,
    TagRemove,
    Unlink,
    Announce,
}

impl Action {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "tag_add" => Action::TagAdd,
            "tag_remove" => Action::TagRemove,
            "unlink" => Action::Unlink,
            "announce" => Action::Announce,
            _ => return None,
        })
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Action::TagAdd => "tag_add",
            Action::TagRemove => "tag_remove",
            Action::Unlink => "unlink",
            Action::Announce => "announce",
        }
    }

    pub(super) fn permission(self) -> &'static str {
        match self {
            Action::TagAdd | Action::TagRemove => "kernel.student.edit",
            Action::Unlink => LINK_REVOKE,
            Action::Announce => crate::modules::announcements::SEND_PERMISSION,
        }
    }

    /// Whether `v`'s Staff member may take this action here.
    pub(super) fn allowed(self, v: &View<'_>) -> bool {
        v.staff.can(self.permission())
            && (self != Action::Announce || v.s.deployment.has_back_office("/announcements/new"))
    }
}

fn get<'a>(pairs: &'a [(String, String)], k: &str) -> &'a str {
    pairs
        .iter()
        .find(|(n, _)| n == k)
        .map_or("", |(_, v)| v.as_str())
}

pub(super) async fn submit(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Form(pairs): Pairs,
) -> Response {
    let v = View::new(&s, &staff);
    let sel = Selection::from_pairs(&pairs);
    let back = sel.list_href();
    let Some(action) = Action::parse(get(&pairs, "action")) else {
        return v.redirect(&back, false, "kernel.students.bulk_none");
    };
    if !action.allowed(&v) {
        return crate::auth::forbidden(&s, &staff, action.permission());
    }
    let cap = if action == Action::Unlink {
        MAX_UNLINK
    } else {
        MAX_BULK
    };
    let chosen = match bulk::resolve(&s.services, &s.config.roster, &sel, cap).await {
        Ok(c) => c,
        Err(e) => return v.error(&e),
    };
    if chosen.total == 0 {
        return v.redirect(&back, false, "kernel.students.bulk_none");
    }
    if chosen.over(cap) {
        let key = match action {
            Action::Unlink => "kernel.students.bulk_unlink_too_many",
            Action::Announce => "kernel.students.bulk_announce_too_many",
            _ => "kernel.students.bulk_too_many",
        };
        return refused(&v, key, chosen.total, &back);
    }
    if action == Action::Announce {
        return announce(&sel, &chosen);
    }
    let tag = match action {
        Action::Unlink => String::new(),
        _ => match super::tags::valid_name(get(&pairs, "tag")) {
            Some(t) => t.to_string(),
            None => return v.redirect(&back, false, "kernel.students.tag_invalid"),
        },
    };
    let s_ = &s.services;
    let reach = match action {
        Action::TagAdd => write::holding(s_, &chosen.ids, &tag)
            .await
            .map(|h| chosen.ids.len().saturating_sub(h)),
        Action::TagRemove => write::holding(s_, &chosen.ids, &tag).await,
        _ => write::linked(s_, &chosen.ids).await,
    };
    let reach = match reach {
        Ok(n) => n,
        Err(e) => return v.error(&e),
    };
    let confirm = get(&pairs, "confirm") == "yes";
    let seen = get(&pairs, "n").parse::<usize>().ok();
    let typed = pairs
        .iter()
        .find(|(k, _)| k == TYPED)
        .map(|(_, t)| t.as_str());
    let typed_ok =
        action != Action::Unlink || reach <= 1 || crate::ui::typed_count_ok(typed, reach as u64);
    let step = Review {
        sel: &sel,
        action,
        tag: &tag,
        chosen: &chosen,
        reach,
        changed: confirm && seen != Some(reach),
        wrong: confirm && seen == Some(reach) && !typed_ok,
    };
    if !confirm || step.changed || step.wrong || reach == 0 {
        return review(&v, &step);
    }
    let actor = Actor::staff(&staff);
    let d = &s.deployment;
    let done = match action {
        Action::TagAdd => write::add_tag(d, &chosen.ids, &tag, &actor, &ctx)
            .await
            .map(|n| (n, "kernel.students.bulk_tag_added")),
        Action::TagRemove => write::remove_tag(d, &chosen.ids, &tag, &actor, &ctx)
            .await
            .map(|n| (n, "kernel.students.bulk_tag_removed")),
        _ => write::unlink(d, &chosen.ids, &actor, &ctx)
            .await
            .map(|u: Unlinked| {
                let key = if u.unnotified > 0 {
                    "kernel.students.bulk_unlinked_unnotified"
                } else {
                    "kernel.students.bulk_unlinked"
                };
                (u.students, key)
            }),
    };
    match done {
        Ok((0, _)) => v.redirect(&back, true, "kernel.students.bulk_nothing"),
        Ok((_, key)) => v.redirect(&back, true, key),
        Err(e) => v.error(&e),
    }
}

/// "ประกาศถึงที่เลือก": the editor prefilled with the codes when a link
/// can carry them ([`crate::ui::announce_href`]); a whole filter too long
/// for a link travels as the filter (`from=students`), read again on the
/// server by the editor.
fn announce(sel: &Selection, chosen: &Chosen) -> Response {
    let href = crate::ui::announce_href(&chosen.codes);
    let to = if href.contains('?') || sel.which != Which::Filter {
        href
    } else {
        let q = super::form::urlencode(&sel.q);
        let m = if sel.missing { "&missing=1" } else { "" };
        format!("/admin/announcements/new?scope=students&from=students&q={q}{m}")
    };
    let mut resp = (StatusCode::SEE_OTHER, [(axum::http::header::LOCATION, to)]).into_response();
    resp.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    resp
}

/// A refusal page: the problem with the count, what to do, and the way
/// back.
fn refused(v: &View<'_>, key: &str, n: usize, back: &str) -> Response {
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

/// One review step.
struct Review<'a> {
    sel: &'a Selection,
    action: Action,
    tag: &'a str,
    chosen: &'a Chosen,
    /// Students the action changes.
    reach: usize,
    /// The count differs from the one confirmed.
    changed: bool,
    /// The typed number did not match.
    wrong: bool,
}

fn review(v: &View<'_>, r: &Review<'_>) -> Response {
    let title = v.t("kernel.students.bulk_review");
    let a = r.action.as_str();
    let n = crate::ui::number(r.reach);
    let fill = |key: &str, n: &str| v.t(key).replace("{n}", n).replace("{tag}", r.tag);
    let skipped = r.chosen.ids.len().saturating_sub(r.reach);
    let typed = r.action == Action::Unlink && r.reach > 1;
    let codes = &r.chosen.codes;
    let more = codes.len().saturating_sub(SHOWN_CODES);
    let status = if r.wrong {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::OK
    };
    let body = html! {
        div class="card narrow bulk-review" {
            h1 { (title) }
            @if r.changed {
                p class="blocked-reason" role="status" { (v.t("kernel.students.bulk_changed")) }
            }
            p class="reach" id="bulk-reach" { strong { (fill(&format!("kernel.students.bulk_review_{a}"), &n)) } }
            @if skipped > 0 {
                p class="muted" { (fill(&format!("kernel.students.bulk_skip_{a}"), &crate::ui::number(skipped))) }
            }
            h2 class="codes-title" { (v.t("kernel.students.bulk_codes")) }
            p class="codes" {
                (codes.iter().take(SHOWN_CODES).cloned().collect::<Vec<_>>().join(", "))
                @if more > 0 { " " (v.t("kernel.students.bulk_more").replace("{n}", &crate::ui::number(more))) }
            }
            form id="bulk-confirm" method="post" action=(PATH) {
                (v.staff.csrf_field())
                (r.sel.hidden())
                input type="hidden" name="action" value=(a);
                input type="hidden" name="tag" value=(r.tag);
                input type="hidden" name="n" value=(r.reach);
                input type="hidden" name="confirm" value="yes";
                @if typed && !r.changed { (typed_field(v, &n, r.wrong)) }
                div class="actions" {
                    @if r.reach == 0 {
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

/// The HTMX count of the selection: `#bulk-count`, a polite live region.
pub(super) async fn count(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Form(pairs): Pairs,
) -> Response {
    let v = View::new(&s, &staff);
    let sel = Selection::from_pairs(&pairs);
    let n = match sel.which {
        Which::Picked => Ok(sel.picked.len()),
        _ => bulk::resolve(&s.services, &s.config.roster, &sel, MAX_BULK)
            .await
            .map(|c| c.total),
    };
    match n {
        Ok(n) => (StatusCode::OK, Html(count_text(&v, n).into_string())).into_response(),
        Err(e) => v.error(&e),
    }
}

/// `#bulk-count` for `n` Students selected: a polite live region whose
/// text the HTMX count replaces ([`count_text`]), so the region itself
/// stays and is announced.
pub(super) fn count_line(v: &View<'_>, n: usize) -> Markup {
    html! { p id="bulk-count" class="bulk-count" aria-live="polite" { (count_text(v, n)) } }
}

/// The text of `#bulk-count`.
fn count_text(v: &View<'_>, n: usize) -> Markup {
    html! {
        @if n == 0 {
            (v.t("kernel.students.bulk_count_none"))
        } @else {
            strong { (v.t("kernel.students.bulk_count").replace("{n}", &crate::ui::number(n))) }
        }
    }
}
