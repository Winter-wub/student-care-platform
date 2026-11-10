//! `POST /admin/students/bulk` (#145): a bulk action on the list's
//! selection ([`super::bulk`]). `action` names it: `tag_add`, `tag_remove`
//! and `unlink` show a review step naming the Students who will change
//! (and, for an unlink of more than one Student, ask for that number
//! typed, #120/#135), then act on `confirm=yes` only if the same Students
//! would change (the review's `set` token); `announce` opens the
//! announcement editor with the selection's active Students as a
//! hand-picked Audience. Each action checks its own Permission here, on
//! top of the route's `kernel.student.view`.
//!
//! `POST /admin/students/bulk/count` is the HTMX count of the selection:
//! the text of `#bulk-count`.

use std::collections::HashSet;

use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use super::bulk::{self, Chosen, MAX_BULK, MAX_UNLINK, Selection, Which};
use super::bulk_write as write;
use super::page_bulk_review::{Review, refused, review};
use super::view::View;
use crate::kernel::LINK_REVOKE;
use crate::{Actor, AppState, CurrentStaff, RequestCtx};

/// The route of the bulk form.
pub(super) const PATH: &str = "/admin/students/bulk";
/// The route of its HTMX count.
pub(super) const COUNT_PATH: &str = "/admin/students/bulk/count";
/// The typed confirmation field.
pub(super) const TYPED: &str = "typed";

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
        return v.redirect(&back, false, "kernel.students.bulk_unknown");
    };
    if !action.allowed(&v) {
        return crate::auth::forbidden(&s, &staff, action.permission());
    }
    let announcing = action == Action::Announce;
    let roster = &s.config.roster;
    let chosen = match bulk::resolve(&s.services, roster, &sel, MAX_BULK, announcing).await {
        Ok(c) => c,
        Err(e) => return v.error(&e),
    };
    if chosen.total == 0 {
        let key = if announcing {
            "kernel.students.bulk_none_active"
        } else {
            "kernel.students.bulk_none"
        };
        return v.redirect(&back, false, key);
    }
    if chosen.over(MAX_BULK) {
        let key = if announcing {
            "kernel.students.bulk_announce_too_many"
        } else {
            "kernel.students.bulk_too_many"
        };
        return refused(&v, key, chosen.total, &back);
    }
    if announcing {
        return announce(&sel, &chosen);
    }
    let tag = match action {
        Action::Unlink => String::new(),
        _ => match super::tags::valid_name(get(&pairs, "tag")) {
            Some(t) => t.to_string(),
            None => return v.redirect(&back, false, "kernel.students.tag_invalid"),
        },
    };
    // The Students the action would change now, in list order.
    let changing = match changing(&s, action, &chosen, &tag).await {
        Ok(c) => c,
        Err(e) => return v.error(&e),
    };
    if action == Action::Unlink && changing.len() > MAX_UNLINK {
        let key = "kernel.students.bulk_unlink_too_many";
        return refused(&v, key, changing.len(), &back);
    }
    let (ids, codes): (Vec<String>, Vec<String>) = changing.into_iter().unzip();
    let token = bulk::set_token(&ids);
    let confirm = get(&pairs, "confirm") == "yes";
    let same =
        get(&pairs, "n").parse::<usize>().ok() == Some(ids.len()) && get(&pairs, "set") == token;
    let typed = pairs
        .iter()
        .find(|(k, _)| k == TYPED)
        .map(|(_, t)| t.as_str());
    let typed_ok = action != Action::Unlink
        || ids.len() <= 1
        || crate::ui::typed_count_ok(typed, ids.len() as u64);
    let step = Review {
        sel: &sel,
        action,
        tag: &tag,
        selected: &chosen.ids,
        codes: &codes,
        token: &token,
        changed: confirm && !same,
        wrong: confirm && same && !typed_ok,
    };
    if !confirm || step.changed || step.wrong || ids.is_empty() {
        return review(&v, &step);
    }
    act(&v, action, &ids, &tag, &staff, &ctx, &back).await
}

/// The (id, code) of each chosen Student `action` would change.
async fn changing(
    s: &AppState,
    action: Action,
    chosen: &Chosen,
    tag: &str,
) -> crate::Result<Vec<(String, String)>> {
    let sv = &s.services;
    let hit: HashSet<String> = match action {
        Action::TagAdd | Action::TagRemove => write::holding(sv, &chosen.ids, tag).await?,
        _ => write::linked(sv, &chosen.ids).await?,
    }
    .into_iter()
    .collect();
    // Adding changes those without the Tag; the others, those with it.
    let keep = |id: &String| hit.contains(id) != (action == Action::TagAdd);
    Ok(chosen
        .ids
        .iter()
        .zip(&chosen.codes)
        .filter(|(id, _)| keep(id))
        .map(|(id, code)| (id.clone(), code.clone()))
        .collect())
}

/// Runs a confirmed action on `ids` and goes back to the list.
async fn act(
    v: &View<'_>,
    action: Action,
    ids: &[String],
    tag: &str,
    staff: &CurrentStaff,
    ctx: &RequestCtx,
    back: &str,
) -> Response {
    let actor = Actor::staff(staff);
    let d = &v.s.deployment;
    let done = match action {
        Action::TagAdd => write::add_tag(d, ids, tag, &actor, ctx)
            .await
            .map(|n| (n, "kernel.students.bulk_tag_added")),
        Action::TagRemove => write::remove_tag(d, ids, tag, &actor, ctx)
            .await
            .map(|n| (n, "kernel.students.bulk_tag_removed")),
        _ => write::unlink(d, ids, &actor, ctx).await.map(|u| {
            let key = if u.partial {
                PARTIAL
            } else if u.unnotified > 0 {
                "kernel.students.bulk_unlinked_unnotified"
            } else {
                "kernel.students.bulk_unlinked"
            };
            (u.students, key)
        }),
    };
    match done {
        Ok((_, PARTIAL)) => v.redirect(back, false, PARTIAL),
        Ok((0, _)) => v.redirect(back, true, "kernel.students.bulk_nothing"),
        Ok((_, key)) => v.redirect(back, true, key),
        Err(e) => v.error(&e),
    }
}

/// The flash of an unlink that stopped after some Students.
const PARTIAL: &str = "kernel.students.bulk_unlinked_partial";

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

/// The HTMX count of the selection: the text of `#bulk-count`, counted
/// with `count(*)` only.
pub(super) async fn count(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Form(pairs): Pairs,
) -> Response {
    let v = View::new(&s, &staff);
    let sel = Selection::from_pairs(&pairs);
    match bulk::count(&s.services, &s.config.roster, &sel).await {
        Ok(n) => (StatusCode::OK, Html(count_text(&v, n).into_string())).into_response(),
        Err(e) => v.error(&e),
    }
}

/// `#bulk-count`: a polite live region whose text the HTMX count replaces
/// ([`count_text`]), so the region itself stays and is announced. It
/// starts with a neutral hint: without JS it never changes, so it must
/// not say that nobody is selected.
pub(super) fn count_line(v: &View<'_>) -> Markup {
    html! {
        p id="bulk-count" class="bulk-count" aria-live="polite" {
            (v.t("kernel.students.bulk_count_hint"))
        }
    }
}

/// The text of `#bulk-count` for `n` Students selected.
fn count_text(v: &View<'_>, n: usize) -> Markup {
    html! {
        @if n == 0 {
            (v.t("kernel.students.bulk_count_none"))
        } @else {
            strong { (v.t("kernel.students.bulk_count").replace("{n}", &crate::ui::number(n))) }
        }
    }
}
