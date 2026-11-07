//! The detail page's POSTs: confirm a draft, cancel a schedule, "ส่งซ้ำ"
//! and retract (the last two ask for confirmation first). Each carries
//! the version the page showed and answers 303 + flash.

use axum::Form;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::html;

use super::service;
use super::view::View;
use crate::{Actor, AppState, CurrentStaff, Error, RequestCtx};

type Posted = Form<Vec<(String, String)>>;

/// What was posted: the version and whether the confirmation was given.
fn fields(Form(pairs): Posted) -> (i64, bool) {
    let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    (
        get("version").and_then(|v| v.parse().ok()).unwrap_or(-1),
        get("confirm") == Some("yes"),
    )
}

fn back(id: &str) -> String {
    format!("/admin/announcements/{id}")
}

fn done(v: &View<'_>, id: &str, r: crate::Result<()>, ok_key: &str) -> Response {
    match r {
        Ok(()) => v.redirect(&back(id), true, ok_key),
        Err(Error::Conflict(_)) => v.redirect(&back(id), false, "announcements.flash.conflict"),
        Err(Error::Invalid(_)) => v.not_found(),
        Err(e) => v.error(&e),
    }
}

/// The confirmation page re-posting `version` with `confirm=yes`.
fn ask(v: &View<'_>, id: &str, path: &str, text_key: &str, version: i64) -> Response {
    let title = v.t("announcements.page.confirm");
    let body = html! {
        div class="card narrow" {
            h1 { (title) }
            p { (v.t(text_key)) }
            form method="post" action=(format!("{}/{path}", back(id))) {
                (v.staff.csrf_field())
                input type="hidden" name="version" value=(version);
                input type="hidden" name="confirm" value="yes";
                div class="actions" {
                    button type="submit" { (title) }
                    a class="btn secondary" href=(back(id)) { (v.t("announcements.page.cancel")) }
                }
            }
        }
    };
    v.page(StatusCode::OK, &title, body)
}

pub(super) async fn confirm(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    form: Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let (version, _) = fields(form);
    let r = service::confirm(&s.deployment, &id, version, &Actor::staff(&staff), &ctx).await;
    match r {
        Ok(true) => v.redirect(&back(&id), true, "announcements.flash.scheduled"),
        Ok(false) => v.redirect(&back(&id), true, "announcements.flash.sending"),
        Err(e) => done(&v, &id, Err(e), ""),
    }
}

pub(super) async fn cancel(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    form: Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let (version, _) = fields(form);
    let r = service::cancel(&s.deployment, &id, version, &Actor::staff(&staff), &ctx).await;
    done(&v, &id, r, "announcements.flash.cancelled")
}

pub(super) async fn resend(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    form: Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let (version, yes) = fields(form);
    if !yes {
        return ask(
            &v,
            &id,
            "resend",
            "announcements.page.resend_confirm",
            version,
        );
    }
    let r = service::resend(&s.deployment, &id, version, &Actor::staff(&staff), &ctx).await;
    if matches!(&r, Err(Error::Conflict(m)) if m == "announcement expired") {
        return v.redirect(&back(&id), false, "announcements.flash.expired");
    }
    done(&v, &id, r, "announcements.flash.resending")
}

pub(super) async fn retract(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    form: Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let (version, yes) = fields(form);
    if !yes {
        return ask(
            &v,
            &id,
            "retract",
            "announcements.page.retract_confirm",
            version,
        );
    }
    let r = service::retract(&s.deployment, &id, version, &Actor::staff(&staff), &ctx).await;
    done(&v, &id, r, "announcements.flash.retracted")
}
