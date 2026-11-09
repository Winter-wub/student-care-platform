//! `/admin/staff` (`kernel.staff.manage`): the Staff list and the create
//! form. A new account gets a generated temporary password, shown once on
//! the result page and never stored or logged; the first sign-in must
//! change it (`must_change_password`). The account page is
//! `staff_detail.rs`.

use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};

use super::view::{View, cut};
use crate::auth::roles::{self, RoleRow};
use crate::auth::staff_admin::{self, StaffRow};
use crate::message::Locale;
use crate::{Actor, AppState, CurrentStaff, Error, NewStaff, RequestCtx, create_staff};

/// Status chips of an account: disabled, locked, must change.
pub(super) fn chips(v: &View<'_>, r: &StaffRow) -> Markup {
    let now = v.s.services.now_ms();
    html! {
        @if r.status == "active" {
            span class="chip ok" { (v.t("kernel.admin.staff.active")) }
        } @else {
            span class="chip" { (v.t("kernel.admin.staff.disabled")) }
        }
        @if let Some(until) = r.locked_until.filter(|u| *u > now) {
            " " span class="chip override" { (v.t("kernel.admin.staff.locked_until")) " " (v.time(until)) }
        }
        @if r.must_change_password {
            " " span class="chip" { (v.t("kernel.admin.staff.must_change")) }
        }
    }
}

pub(super) async fn list(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let v = View::new(&s, &staff);
    let rows = match staff_admin::list(&s.services).await {
        Ok(r) => r,
        Err(e) => return v.error(&e),
    };
    let title = v.t("kernel.admin.staff.title");
    let l = |k: &str| v.t(k);
    let body = html! {
        h1 { (title) }
        p class="actions" {
            a class="btn" href="/admin/staff/new" { (l("kernel.admin.staff.create")) }
            a class="btn secondary" href="/admin/roles" { (l("kernel.admin.roles.title")) }
        }
        p class="muted count" { (l("kernel.admin.total")) " " (crate::ui::number(rows.len())) }
        table class="rows staff" {
            thead { tr {
                th { (l("kernel.admin.staff.name")) }
                th { (l("kernel.admin.staff.email")) }
                th { (l("kernel.admin.staff.status")) }
                th { (l("kernel.admin.staff.roles")) }
            } }
            tbody {
                @for r in &rows {
                    tr {
                        td data-label=(l("kernel.admin.staff.name")) {
                            a class="name" href=(format!("/admin/staff/{}", r.id)) { (r.display_name) }
                        }
                        td data-label=(l("kernel.admin.staff.email")) { (r.email) }
                        td data-label=(l("kernel.admin.staff.status")) { (chips(&v, r)) }
                        td data-label=(l("kernel.admin.staff.roles")) {
                            @for role in &r.roles { span class="chip" { (v.role(role)) } " " }
                        }
                    }
                }
            }
        }
    };
    v.page(StatusCode::OK, &title, body)
}

/// What the create form holds.
#[derive(Debug, Default)]
struct Fields {
    email: String,
    display_name: String,
    locale: String,
    roles: Vec<String>,
}

pub(super) async fn new_form(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let v = View::new(&s, &staff);
    let f = Fields {
        locale: s.config.default_locale.0.clone(),
        ..Fields::default()
    };
    render(&v, &f, None, StatusCode::OK).await
}

/// Role checkboxes named `role` (the stored name), `held` checked, each
/// labelled as Staff read it (#125).
pub(super) fn role_boxes(v: &View<'_>, all: &[RoleRow], held: &[String]) -> Markup {
    html! {
        fieldset class="checks" {
            @for r in all {
                label class="check" {
                    input type="checkbox" name="role" value=(r.name) checked[held.contains(&r.name)];
                    " " (v.role(&r.name))
                }
            }
        }
    }
}

async fn render(v: &View<'_>, f: &Fields, error: Option<&str>, status: StatusCode) -> Response {
    let all = match roles::list(&v.s.services).await {
        Ok(r) => r,
        Err(e) => return v.error(&e),
    };
    let title = v.t("kernel.admin.staff.create");
    let l = |k: &str| v.t(k);
    let locales = super::settings::locales(&v.s.catalog, &v.s.config);
    let body = html! {
        p { a href="/admin/staff" { "← " (l("kernel.admin.staff.back")) } }
        h1 { (title) }
        form method="post" action="/admin/staff/new" class="card narrow" {
            (v.staff.csrf_field())
            @if let Some(e) = error { p class="field-error" role="alert" { (l(e)) } }
            div class="field" {
                label for="staff-email" { (l("kernel.admin.staff.email")) }
                input id="staff-email" type="email" name="email" value=(f.email) required autocomplete="off";
            }
            div class="field" {
                label for="staff-name" { (l("kernel.admin.staff.name")) }
                input id="staff-name" name="display_name" value=(f.display_name) required maxlength="200";
            }
            div class="field" {
                label for="staff-locale" { (l("kernel.admin.staff.locale")) }
                select id="staff-locale" name="locale" {
                    @for loc in &locales {
                        option value=(loc) selected[&f.locale == loc] { (v.s.catalog.locale_name(&v.staff.locale, loc)) }
                    }
                }
            }
            div class="field" {
                label { (l("kernel.admin.staff.roles")) }
                (role_boxes(v, &all, &f.roles))
            }
            p class="muted" { (l("kernel.admin.staff.temp_help")) }
            button type="submit" { (title) }
        }
    };
    v.page(status, &title, body)
}

pub(super) async fn create(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let v = View::new(&s, &staff);
    let mut f = Fields::default();
    for (k, val) in pairs {
        match k.as_str() {
            "email" => f.email = cut(&val, 254),
            "display_name" => f.display_name = cut(&val, 200),
            "locale" if super::settings::locales(&s.catalog, &s.config).contains(&val) => {
                f.locale = val
            }
            "role" => f.roles.push(val),
            _ => {}
        }
    }
    let invalid = if f.display_name.is_empty() || f.display_name.chars().any(char::is_control) {
        Some("kernel.admin.staff.err_name")
    } else if !f.email.contains('@') || f.email.chars().any(char::is_whitespace) {
        Some("kernel.admin.staff.err_email")
    } else {
        None
    };
    if let Some(e) = invalid {
        return render(&v, &f, Some(e), StatusCode::UNPROCESSABLE_ENTITY).await;
    }
    let password = match staff_admin::temporary_password(&s.services) {
        Ok(p) => p,
        Err(e) => return v.error(&e),
    };
    let locale = if f.locale.is_empty() {
        s.config.default_locale.clone()
    } else {
        Locale(f.locale.clone())
    };
    let mut new = NewStaff::new(&f.email, &f.display_name, &password, locale);
    new.roles = f.roles.clone();
    new.must_change_password = true;
    let actor = Actor::staff(&staff);
    match create_staff(&s.services, &actor, new, &ctx).await {
        Ok(id) => shown_once(&v, &id, &f.email.trim().to_lowercase(), &password, false),
        Err(Error::Invalid(_)) => {
            let key = match refusal(&s.services, &f).await {
                Ok(k) => k,
                Err(e) => return v.error(&e),
            };
            render(&v, &f, Some(key), StatusCode::UNPROCESSABLE_ENTITY).await
        }
        Err(e) => v.error(&e),
    }
}

/// Why `create_staff` refused `f` (`Error::Invalid`), read from the
/// database rather than from the error text: a Role gone, the email
/// taken, else the email itself.
async fn refusal(s: &crate::Services, f: &Fields) -> crate::Result<&'static str> {
    if roles::check_exist(s, &f.roles).await.is_err() {
        return Ok("kernel.admin.staff.err_roles");
    }
    let email = f.email.trim().to_lowercase();
    let taken =
        s.db.query(crate::Statement::new("SELECT 1 FROM staff WHERE email = ?").bind(email))
            .await?;
    Ok(if taken.is_empty() {
        "kernel.admin.staff.err_email"
    } else {
        "kernel.admin.staff.err_taken"
    })
}

/// The page showing a temporary password once (create or reset).
pub(super) fn shown_once(
    v: &View<'_>,
    id: &str,
    email: &str,
    password: &str,
    reset: bool,
) -> Response {
    let l = |k: &str| v.t(k);
    let title = l(if reset {
        "kernel.admin.staff.reset_done"
    } else {
        "kernel.admin.staff.created"
    });
    let body = html! {
        div class="card narrow" {
            h1 { (title) }
            dl class="facts" {
                dt { (l("kernel.admin.staff.email")) } dd { (email) }
                dt { (l("kernel.admin.staff.temp_password")) }
                dd { code class="secret" id="temp-password" { (password) } }
            }
            p class="flash err" { (l("kernel.admin.staff.shown_once")) }
            a class="btn" href=(format!("/admin/staff/{id}")) { (l("kernel.admin.staff.open")) }
        }
    };
    v.page(StatusCode::OK, &title, body)
}
