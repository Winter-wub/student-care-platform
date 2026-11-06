//! The Students detail page's POSTs: manual Tags, status (with a
//! confirmation step), Manual Link and revoke (with a confirmation step).
//! Each answers with a 303 back to the page and a flash.

use axum::Form;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::html;

use super::form::FormMap;
use super::view::View;
use super::{service, tags};
use crate::{Actor, AppState, CurrentStaff, Error, RequestCtx, links};

/// The posted form as a map.
type Posted = Form<Vec<(String, String)>>;

fn map(Form(pairs): Posted) -> FormMap {
    pairs.into_iter().collect()
}

fn back(id: &str) -> String {
    format!("/admin/students/{id}")
}

/// A confirmation page: `text`, then a form re-posting `fields` plus
/// `confirm=yes` to `action`.
fn confirm(
    v: &View<'_>,
    text_key: &str,
    action: &str,
    fields: &[(&str, &str)],
    id: &str,
) -> Response {
    let title = v.t("kernel.students.confirm");
    let body = html! {
        div class="card narrow" {
            h1 { (title) }
            p { (v.t(text_key)) }
            form method="post" action=(action) {
                (v.staff.csrf_field())
                @for (k, val) in fields { input type="hidden" name=(k) value=(val); }
                input type="hidden" name="confirm" value="yes";
                div class="actions" {
                    button type="submit" { (title) }
                    a class="btn secondary" href=(back(id)) { (v.t("kernel.students.cancel")) }
                }
            }
        }
    };
    v.page(StatusCode::OK, &title, body)
}

fn confirmed(f: &FormMap) -> bool {
    f.get("confirm").map(String::as_str) == Some("yes")
}

pub(super) async fn add_tag(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    form: Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let f = map(form);
    let name = f.get("name").map(String::as_str).unwrap_or_default();
    let actor = Actor::staff(&staff);
    match tags::add_manual_tag(&s.deployment, &id, name, &actor, &ctx).await {
        Ok(()) => v.redirect(&back(&id), true, "kernel.students.tag_added"),
        Err(Error::Conflict(_)) => v.redirect(&back(&id), false, "kernel.students.tag_exists"),
        Err(Error::Invalid(m)) if m == "tag name" => {
            v.redirect(&back(&id), false, "kernel.students.tag_invalid")
        }
        Err(Error::Invalid(_)) => v.not_found(),
        Err(e) => v.error(&e),
    }
}

pub(super) async fn remove_tag(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    form: Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let f = map(form);
    let tag = f.get("tag_id").map(String::as_str).unwrap_or_default();
    let actor = Actor::staff(&staff);
    match tags::remove_manual_tag(&s.deployment, &id, tag, &actor, &ctx).await {
        Ok(()) => v.redirect(&back(&id), true, "kernel.students.tag_removed"),
        Err(Error::Conflict(_)) => v.redirect(&back(&id), false, "kernel.students.conflict"),
        Err(Error::Invalid(_)) => v.not_found(),
        Err(e) => v.error(&e),
    }
}

/// `status=left|active`; without `confirm=yes` it shows the confirmation.
pub(super) async fn status(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    form: Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let f = map(form);
    let to = f.get("status").map(String::as_str).unwrap_or_default();
    let text = match to {
        "left" => "kernel.students.confirm_left",
        "active" => "kernel.students.confirm_active",
        _ => return v.redirect(&back(&id), false, "kernel.students.conflict"),
    };
    if !confirmed(&f) {
        let action = format!("{}/status", back(&id));
        return confirm(&v, text, &action, &[("status", to)], &id);
    }
    let actor = Actor::staff(&staff);
    match service::set_status(&s.deployment, &id, to, &actor, &ctx).await {
        Ok(c) if c.changed && c.revoked > c.notified => {
            v.redirect(&back(&id), true, "kernel.students.status_saved_unnotified")
        }
        Ok(c) if c.changed => v.redirect(&back(&id), true, "kernel.students.status_saved"),
        Ok(_) => v.redirect(&back(&id), true, "kernel.students.no_change"),
        Err(Error::Conflict(_)) => v.redirect(&back(&id), false, "kernel.students.conflict"),
        Err(Error::Invalid(_)) => v.not_found(),
        Err(e) => v.error(&e),
    }
}

/// `channel` + `external_id` → [`links::manual_link`].
pub(super) async fn manual_link(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    form: Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let f = map(form);
    let get = |k: &str| f.get(k).map(String::as_str).unwrap_or_default();
    let actor = Actor::staff(&staff);
    let done = links::manual_link(
        &s.deployment,
        &id,
        get("channel"),
        get("external_id"),
        &actor,
        &ctx,
    )
    .await;
    match done {
        Ok(c) if c.notified => v.redirect(&back(&id), true, "kernel.students.linked"),
        Ok(_) => v.redirect(&back(&id), true, "kernel.students.linked_unnotified"),
        Err(Error::Invalid(_)) => v.redirect(&back(&id), false, "kernel.students.link_invalid"),
        Err(Error::Conflict(_)) => v.redirect(&back(&id), false, "kernel.students.link_conflict"),
        Err(e) => v.error(&e),
    }
}

/// Revokes Link `link_id` of the Student; without `confirm=yes` it shows
/// the confirmation. A Link of another Student is not found.
pub(super) async fn revoke(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path((id, link_id)): Path<(String, String)>,
    form: Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let f = map(form);
    let owned = match super::read::links(&s.services, &id).await {
        Ok(ls) => ls.into_iter().any(|l| l.id == link_id),
        Err(e) => return v.error(&e),
    };
    if !owned {
        return v.not_found();
    }
    if !confirmed(&f) {
        let action = format!("{}/links/{link_id}/revoke", back(&id));
        return confirm(&v, "kernel.students.confirm_revoke", &action, &[], &id);
    }
    let actor = Actor::staff(&staff);
    match links::revoke_by_staff(&s.deployment, &link_id, &actor, &ctx).await {
        Ok(c) if c.notified => v.redirect(&back(&id), true, "kernel.students.revoked"),
        Ok(_) => v.redirect(&back(&id), true, "kernel.students.revoked_unnotified"),
        Err(Error::Conflict(_)) => v.redirect(&back(&id), false, "kernel.students.already_revoked"),
        Err(Error::Invalid(_)) => v.not_found(),
        Err(e) => v.error(&e),
    }
}
