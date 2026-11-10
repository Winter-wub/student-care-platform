//! `GET /admin/announcements/audiences` (#144): the saved Audiences, each
//! with what it selects, a warning when `[roster]` no longer has one of
//! its values, a link that starts an announcement with it, and rename
//! (`POST …/audiences/{id}/rename`) and delete (`GET …/{id}/delete`, a
//! confirm step, then `POST` with `confirm=yes`). A row whose rules do not
//! read says so and can only be deleted.

use axum::Form;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};
use serde::Deserialize;

use super::page_list::audience_summary;
use super::page_saved::{LIST, label_of};
use super::saved::{self, Refused, Saved};
use super::view::View;
use crate::kernel::text::fill;
use crate::{Actor, AppState, CurrentStaff, RequestCtx};

#[derive(Debug, Default, Deserialize)]
pub(super) struct NameForm {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct DeleteForm {
    #[serde(default)]
    confirm: String,
}

pub(super) async fn page(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let v = View::new(&s, &staff);
    let list = match saved::list(&s.services).await {
        Ok(l) => l,
        Err(e) => return v.error(&e),
    };
    let title = v.t("announcements.saved.title");
    let write = v.t("kernel.empty.announcements_action");
    let body = html! {
        p { a class="back" href="/admin/announcements" { "← " (v.t("announcements.nav")) } }
        h1 { (title) }
        p class="muted" { (v.t("announcements.saved.help")) }
        @if list.is_empty() {
            (crate::ui::empty_state(&v.t("announcements.saved.empty"), Some((write.as_str(), "/admin/announcements/new"))))
        } @else {
            ul class="list card saved-list" {
                @for one in &list { (item(&v, one)) }
            }
        }
    };
    v.page(StatusCode::OK, &title, body)
}

fn item(v: &View<'_>, one: &Saved) -> Markup {
    let base = format!("{LIST}/{}", one.id);
    let name_id = format!("name-{}", one.id);
    let updated = html! { (v.t("announcements.saved.updated")) " " (v.time(Some(one.updated_at))) };
    let delete = html! {
        a class="btn secondary" href=(format!("{base}/delete")) { (v.t("announcements.saved.delete")) }
    };
    let Some(a) = &one.audience else {
        return html! {
            li {
                p { strong class="name" { (one.name) } }
                p class="muted" { (updated) }
                p class="note warn-note" role="note" { (v.t("announcements.saved.unreadable")) }
                div class="actions" { (delete) }
            }
        };
    };
    let gone = saved::stale(a, &v.s.config.roster);
    html! {
        li {
            p { strong class="name" { (one.name) } }
            p class="muted" { (audience_summary(v, a)) " · " (updated) }
            @if !gone.is_empty() {
                p class="note warn-note" role="note" {
                    (v.t("announcements.saved.stale_list")) " "
                    @for (i, (key, value)) in gone.iter().enumerate() {
                        @if i > 0 { ", " } span class="id" { (label_of(v, key)) ": " (value) }
                    }
                }
            }
            div class="actions" {
                a class="btn secondary" href=(format!("/admin/announcements/new?audience={}", one.id)) {
                    (v.t("announcements.saved.write_to"))
                }
            }
            details class="saved-edit" {
                summary { (v.t("announcements.saved.edit")) }
                form method="post" action=(format!("{base}/rename")) {
                    (v.staff.csrf_field())
                    div class="field" {
                        label for=(name_id) { (v.t("announcements.saved.new_name")) }
                        input id=(name_id) name="name" required maxlength=(saved::MAX_NAME) value=(one.name);
                    }
                    button type="submit" class="secondary" { (v.t("announcements.saved.rename")) }
                }
                p class="muted" { (v.t("announcements.saved.delete_help")) }
                div class="actions" { (delete) }
            }
        }
    }
}

pub(super) async fn rename(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    Form(form): Form<NameForm>,
) -> Response {
    let v = View::new(&s, &staff);
    let one = match saved::load(&s.services, &id).await {
        Ok(Some(one)) => one,
        Ok(None) => return v.redirect(LIST, false, "announcements.saved.gone"),
        Err(e) => return v.error(&e),
    };
    let Some(name) = saved::clean_name(&form.name) else {
        return v.redirect(LIST, false, "announcements.saved.bad_name");
    };
    if name == one.name {
        // Nothing to write, so nothing to audit.
        return v.redirect(LIST, true, "announcements.saved.unchanged");
    }
    let actor = Actor::staff(&staff);
    match saved::rename(&s.services, &one, &name, &actor, &ctx).await {
        Ok(Ok(())) => v.redirect(LIST, true, "announcements.saved.renamed"),
        Ok(Err(Refused::Taken)) => v.redirect(LIST, false, "announcements.saved.taken"),
        Ok(Err(_)) => v.redirect(LIST, false, "announcements.saved.gone"),
        Err(e) => v.error(&e),
    }
}

/// `GET …/{id}/delete`: the confirm step.
pub(super) async fn delete_page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let one = match saved::load(&s.services, &id).await {
        Ok(Some(one)) => one,
        Ok(None) => return v.redirect(LIST, false, "announcements.saved.gone"),
        Err(e) => return v.error(&e),
    };
    let title = fill(
        &v.t("announcements.saved.delete_title"),
        &[("name", &one.name)],
    );
    let keep = v.t("announcements.saved.keep");
    let body = html! {
        p { a class="back" href=(LIST) { "← " (v.t("announcements.saved.title")) } }
        h1 { (title) }
        (crate::ui::confirm_step(
            v.staff.csrf_field(),
            &format!("{LIST}/{}/delete", one.id),
            &v.t("announcements.saved.delete_what"),
            &v.t("announcements.saved.delete"),
            (keep.as_str(), LIST),
        ))
    };
    v.page(StatusCode::OK, &title, body)
}

/// `POST …/{id}/delete`: deletes only with `confirm=yes` (from the confirm
/// step); without it, back to that step.
pub(super) async fn delete(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    Form(form): Form<DeleteForm>,
) -> Response {
    let v = View::new(&s, &staff);
    let one = match saved::load(&s.services, &id).await {
        Ok(Some(one)) => one,
        Ok(None) => return v.redirect(LIST, false, "announcements.saved.gone"),
        Err(e) => return v.error(&e),
    };
    if form.confirm != "yes" {
        let back = format!("{LIST}/{}/delete", one.id);
        return v.redirect(&back, false, "announcements.saved.delete_what");
    }
    let actor = Actor::staff(&staff);
    match saved::delete(&s.services, &one, &actor, &ctx).await {
        Ok(Ok(())) => v.redirect(LIST, true, "announcements.saved.deleted"),
        Ok(Err(_)) => v.redirect(LIST, false, "announcements.saved.gone"),
        Err(e) => v.error(&e),
    }
}
