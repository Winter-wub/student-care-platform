//! `/admin/staff/{id}` (`kernel.staff.manage`): one account with its
//! Roles (checkboxes), disable / enable, set a temporary password (shown
//! once) and clear the lockout. Every form carries the account's
//! `updated_at` as `version`; a stale form is refused (flash), never
//! written.

use axum::Form;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::html;

use super::staff_pages::{chips, role_boxes, shown_once};
use super::view::View;
use crate::auth::roles;
use crate::auth::staff_admin::{self, Outcome};
use crate::{Actor, AppState, CurrentStaff, Error, RequestCtx};

/// A posted form: `version`, `role` (repeated), `active`.
#[derive(Debug, Default)]
pub(super) struct Posted {
    version: i64,
    roles: Vec<String>,
    /// `None` when the field is missing or not `0` / `1`.
    active: Option<bool>,
}

fn posted(pairs: Vec<(String, String)>) -> Posted {
    let mut p = Posted::default();
    for (k, v) in pairs {
        match k.as_str() {
            "version" => p.version = v.parse().unwrap_or(-1),
            "role" => p.roles.push(v),
            "active" => {
                p.active = match v.as_str() {
                    "1" => Some(true),
                    "0" => Some(false),
                    _ => None,
                }
            }
            _ => {}
        }
    }
    p
}

pub(super) async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let loaded = async {
        Ok::<_, Error>((
            staff_admin::get(&s.services, &id).await?,
            roles::list(&s.services).await?,
        ))
    };
    let (row, all) = match loaded.await {
        Ok((Some(r), all)) => (r, all),
        Ok((None, _)) => return v.not_found("/admin/staff"),
        Err(e) => return v.error(&e),
    };
    let base = format!("/admin/staff/{}", row.id);
    let me = row.id == staff.id;
    let version = row.updated_at.to_string();
    let l = |k: &str| v.t(k);
    let locked = row.locked_until.is_some() || row.failed_logins > 0;
    let form = |path: &str| format!("{base}/{path}");
    let body = html! {
        p { a href="/admin/staff" { "← " (l("kernel.admin.staff.back")) } }
        h1 { (row.display_name) " " (chips(&v, &row)) }
        p class="code" { (row.email) }
        div class="cols" {
            section class="card" {
                h2 { (l("kernel.admin.staff.roles")) }
                form method="post" action=(form("roles")) {
                    (staff.csrf_field())
                    input type="hidden" name="version" value=(version);
                    (role_boxes(&all, &row.roles))
                    p class="muted" { (l("kernel.admin.staff.roles_help")) }
                    button type="submit" { (l("kernel.admin.save")) }
                }
            }
            div {
                section class="card" {
                    h2 { (l("kernel.admin.staff.access")) }
                    @if locked {
                        p { (l("kernel.admin.staff.failed_logins")) " " (row.failed_logins) }
                        form method="post" action=(form("unlock")) {
                            (staff.csrf_field())
                            input type="hidden" name="version" value=(version);
                            button class="secondary" type="submit" { (l("kernel.admin.staff.unlock")) }
                        }
                    }
                    @if !me {
                        form method="post" action=(form("password")) {
                            (staff.csrf_field())
                            input type="hidden" name="version" value=(version);
                            p class="muted" { (l("kernel.admin.staff.reset_help")) }
                            button class="secondary" type="submit" { (l("kernel.admin.staff.reset")) }
                        }
                        form method="post" action=(form("status")) {
                            (staff.csrf_field())
                            input type="hidden" name="version" value=(version);
                            @if row.status == "active" {
                                input type="hidden" name="active" value="0";
                                p class="muted" { (l("kernel.admin.staff.disable_help")) }
                                button type="submit" { (l("kernel.admin.staff.disable")) }
                            } @else {
                                input type="hidden" name="active" value="1";
                                button type="submit" { (l("kernel.admin.staff.enable")) }
                            }
                        }
                    } @else {
                        p class="muted" { (l("kernel.admin.staff.own_account")) }
                    }
                }
            }
        }
    };
    v.page(StatusCode::OK, &row.display_name, body)
}

/// The flash for a refused or stale write.
fn refused(v: &View<'_>, back: &str, o: Outcome) -> Response {
    let key = match o {
        Outcome::Done => "kernel.admin.saved",
        Outcome::Stale => "kernel.admin.conflict",
        Outcome::LastManager => "kernel.admin.staff.last_manager",
        Outcome::OwnAccount => "kernel.admin.staff.own_refused",
    };
    v.redirect(back, o == Outcome::Done, key)
}

pub(super) async fn set_roles(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let v = View::new(&s, &staff);
    let p = posted(pairs);
    let back = format!("/admin/staff/{id}");
    let actor = Actor::staff(&staff);
    match staff_admin::set_roles(&s.services, &actor, &id, &p.roles, p.version, &ctx).await {
        Ok(o) => refused(&v, &back, o),
        Err(Error::Invalid(_)) => v.redirect(&back, false, "kernel.admin.staff.err_roles"),
        Err(e) => v.error(&e),
    }
}

pub(super) async fn set_status(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let v = View::new(&s, &staff);
    let p = posted(pairs);
    let back = format!("/admin/staff/{id}");
    let actor = Actor::staff(&staff);
    let Some(active) = p.active else {
        return v.bad_form();
    };
    match staff_admin::set_active(&s.services, &actor, &id, active, p.version, &ctx).await {
        Ok(o) => refused(&v, &back, o),
        Err(e) => v.error(&e),
    }
}

pub(super) async fn reset_password(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let v = View::new(&s, &staff);
    let p = posted(pairs);
    let back = format!("/admin/staff/{id}");
    let actor = Actor::staff(&staff);
    let row = match staff_admin::get(&s.services, &id).await {
        Ok(Some(r)) => r,
        Ok(None) => return v.not_found("/admin/staff"),
        Err(e) => return v.error(&e),
    };
    match staff_admin::reset_password(&s.services, &actor, &id, p.version, &ctx).await {
        Ok((Outcome::Done, Some(password))) => shown_once(&v, &id, &row.email, &password, true),
        Ok((o, _)) => refused(&v, &back, o),
        Err(e) => v.error(&e),
    }
}

pub(super) async fn unlock(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let v = View::new(&s, &staff);
    let p = posted(pairs);
    let back = format!("/admin/staff/{id}");
    let actor = Actor::staff(&staff);
    match staff_admin::unlock(&s.services, &actor, &id, p.version, &ctx).await {
        Ok(Outcome::Done) => v.redirect(&back, true, "kernel.admin.staff.unlocked"),
        Ok(o) => refused(&v, &back, o),
        Err(e) => v.error(&e),
    }
}
