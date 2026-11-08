//! The detail page's POSTs: confirm a draft, cancel a schedule, "ส่งซ้ำ"
//! and retract (the last two ask for confirmation first). Each carries
//! the version the page showed and answers 303 + flash. A confirm comes
//! from the review step only (#120): without `reviewed` it goes there, and
//! a send to everyone (a confirm or a resend) must carry the reach the
//! page showed, counted again here.

use axum::Form;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::LOCATION;
use axum::response::{IntoResponse, Response};
use maud::html;

use super::form::is_everyone;
use super::page_review as review;
use super::service;
use super::store::{self, Row};
use super::view::View;
use crate::{Actor, AppState, CurrentStaff, Error, RequestCtx};

type Posted = Form<Vec<(String, String)>>;

/// What was posted: the version and whether the confirmation was given.
fn fields(form: Posted) -> (i64, bool) {
    let p = posted(form);
    (p.version, p.yes)
}

/// Every field the actions read.
struct Post {
    version: i64,
    yes: bool,
    reviewed: bool,
    reach: Option<String>,
}

fn posted(Form(pairs): Posted) -> Post {
    let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    Post {
        version: get("version").and_then(|v| v.parse().ok()).unwrap_or(-1),
        yes: get("confirm") == Some("yes"),
        reviewed: get(review::REVIEWED) == Some("1"),
        reach: get(review::REACH).map(str::to_string),
    }
}

/// The row, or the response that ends the request (404 / 500).
async fn row(v: &View<'_>, id: &str) -> Result<Row, Box<Response>> {
    match store::load(&v.s.services, id).await {
        Ok(Some(r)) => Ok(r),
        Ok(None) => Err(Box::new(v.not_found())),
        Err(e) => Err(Box::new(v.error(&e))),
    }
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

/// The confirmation page re-posting `version` with `confirm=yes`; with
/// `everyone` (the Students it reaches), the danger button naming them.
fn ask(
    v: &View<'_>,
    id: &str,
    path: &str,
    text_key: &str,
    version: i64,
    everyone: Option<usize>,
) -> Response {
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
                    @if let Some(n) = everyone {
                        (review::everyone_button(v, n))
                    } @else {
                        button type="submit" { (title) }
                    }
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
    let p = posted(form);
    let to_review = review::path(&id);
    if !p.reviewed {
        // The detail page and older pages land on the review step.
        return (StatusCode::SEE_OTHER, [(LOCATION, to_review)]).into_response();
    }
    let row = match row(&v, &id).await {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    if row.status == "draft" {
        match review::everyone_ok(&s.deployment, &row, p.reach.as_deref()).await {
            Ok(Some(false)) => {
                return v.redirect(&to_review, false, "announcements.review.changed");
            }
            Ok(_) => {}
            Err(e) => return v.error(&e),
        }
    }
    let version = p.version;
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
    let p = posted(form);
    let row = match row(&v, &id).await {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    // A new round to everyone: the danger button and its reach (#120).
    let everyone = if is_everyone(&row.audience) {
        match crate::audience::count(&s.deployment, &row.audience).await {
            Ok(c) => Some(c.students),
            Err(e) => return v.error(&e),
        }
    } else {
        None
    };
    let stale = everyone.is_some_and(|n| p.reach.as_deref() != Some(n.to_string().as_str()));
    if !p.yes || stale {
        let key = if p.yes {
            "announcements.review.changed"
        } else {
            "announcements.page.resend_confirm"
        };
        return ask(&v, &id, "resend", key, p.version, everyone);
    }
    let version = p.version;
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
            None,
        );
    }
    let r = service::retract(&s.deployment, &id, version, &Actor::staff(&staff), &ctx).await;
    done(&v, &id, r, "announcements.flash.retracted")
}
