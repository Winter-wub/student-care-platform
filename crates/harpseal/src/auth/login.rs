//! `/admin/login`, `/admin/logout` and `/admin/password`.

use std::collections::HashMap;

use axum::Form;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use super::attempt::attempt;
use super::guard::{bare_page, error_page, redirect};
use super::{
    ABSOLUTE_MS, CSRF_FIELD, CurrentStaff, HOME_PATH, LOGIN_CSRF_COOKIE, LOGIN_PATH,
    MIN_PASSWORD_CHARS, PASSWORD_PATH, SESSION_COOKIE,
};
use super::{check_password_rules, cookies, session, staff};
use crate::AppState;
use crate::audit::{self, Actor, RequestCtx, Target};

type Fields = HashMap<String, String>;

fn field<'a>(f: &'a Fields, name: &str) -> &'a str {
    f.get(name).map(String::as_str).unwrap_or_default()
}

/// `GET /admin/login`: the form, with a fresh double-submit token when the
/// browser has none.
pub(crate) async fn login_form(State(s): State<AppState>, headers: HeaderMap) -> Response {
    let (token, cookie) = match cookies::get(&headers, LOGIN_CSRF_COOKIE) {
        Some(t) => (t, None),
        None => match s.services.random_hex(16) {
            Ok(t) => {
                let c = cookies::set(LOGIN_CSRF_COOKIE, &t, 3600, s.config.staff.cookie_secure);
                (t, Some(c))
            }
            Err(e) => return error_page(&s, &e),
        },
    };
    let flash = cookies::read_flash(&headers).map(|(ok, key)| (ok, s.t(&key)));
    let mut resp = login_page(&s, StatusCode::OK, &token, "", flash);
    if let Some(c) = cookie {
        resp.headers_mut().append(header::SET_COOKIE, c);
    }
    if flash_present(&headers) {
        let clear = cookies::clear(super::FLASH_COOKIE, s.config.staff.cookie_secure);
        resp.headers_mut().append(header::SET_COOKIE, clear);
    }
    resp
}

fn flash_present(headers: &HeaderMap) -> bool {
    cookies::get(headers, super::FLASH_COOKIE).is_some()
}

fn login_page(
    s: &AppState,
    status: StatusCode,
    csrf: &str,
    email: &str,
    flash: Option<(bool, String)>,
) -> Response {
    let title = s.t("kernel.auth.login_title");
    let body = html! {
        div class="card narrow" {
            h1 { (title) }
            (flash_box(&flash))
            form method="post" action=(LOGIN_PATH) {
                input type="hidden" name=(CSRF_FIELD) value=(csrf);
                div class="field" {
                    label for="email" { (s.t("kernel.auth.email")) }
                    input id="email" name="email" type="email" autocomplete="username"
                        required value=(email);
                }
                div class="field" {
                    label for="password" { (s.t("kernel.auth.password")) }
                    input id="password" name="password" type="password"
                        autocomplete="current-password" required;
                }
                button class="wide" type="submit" { (s.t("kernel.auth.sign_in")) }
            }
        }
    };
    let page = crate::ui::bare(s, &title, body);
    (status, Html(page.into_string())).into_response()
}

fn flash_box(flash: &Option<(bool, String)>) -> Markup {
    html! {
        @if let Some((ok, text)) = flash {
            (crate::ui::flash(&crate::ui::Flash { ok: *ok, text }))
        }
    }
}

/// `POST /admin/login`.
pub(crate) async fn login_submit(
    State(s): State<AppState>,
    ctx: RequestCtx,
    headers: HeaderMap,
    Form(f): Form<Fields>,
) -> Response {
    let secure = s.config.staff.cookie_secure;
    let expected = cookies::get(&headers, LOGIN_CSRF_COOKIE).unwrap_or_default();
    let sent = field(&f, CSRF_FIELD);
    if expected.is_empty() || !cookies::ct_eq(sent.as_bytes(), expected.as_bytes()) {
        return bare_page(&s, StatusCode::FORBIDDEN, &s.t("kernel.auth.csrf_failed"));
    }
    let email = field(&f, "email");
    let accepted = match attempt(&s.services, email, field(&f, "password"), &ctx).await {
        Ok(a) => a,
        Err(e) => return error_page(&s, &e),
    };
    let Some(ok) = accepted else {
        let flash = Some((false, s.t("kernel.auth.invalid")));
        return login_page(&s, StatusCode::UNAUTHORIZED, &expected, email, flash);
    };
    let token = match session::create(&s.services, &ok.staff_id, &ctx).await {
        Ok(t) => t,
        Err(e) => return error_page(&s, &e),
    };
    let to = if ok.must_change_password {
        PASSWORD_PATH
    } else {
        HOME_PATH
    };
    let mut resp = (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response();
    let h = resp.headers_mut();
    h.append(
        header::SET_COOKIE,
        cookies::set(SESSION_COOKIE, &token, ABSOLUTE_MS / 1000, secure),
    );
    h.append(
        header::SET_COOKIE,
        cookies::clear(LOGIN_CSRF_COOKIE, secure),
    );
    resp
}

/// `POST /admin/logout` (guarded: session and CSRF already checked).
pub(crate) async fn logout(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    req: Request,
) -> Response {
    let secure = s.config.staff.cookie_secure;
    if let Some(token) = cookies::get(req.headers(), SESSION_COOKIE) {
        let target = Target::new("staff", staff.id.clone());
        let done = async {
            session::delete(&s.services, &token).await?;
            let actor = Actor::staff(&staff);
            audit::record(
                &s.services,
                &actor,
                "staff.logout",
                Some(&target),
                None,
                None,
                &ctx,
            )
            .await
        };
        if let Err(e) = done.await {
            return error_page(&s, &e);
        }
    }
    let mut resp = redirect(
        &req,
        LOGIN_PATH,
        Some(cookies::clear(SESSION_COOKIE, secure)),
    );
    let flash: HeaderValue = cookies::flash(true, "kernel.auth.signed_out", secure);
    resp.headers_mut().append(header::SET_COOKIE, flash);
    resp
}

/// `GET /admin/password`: the change form (the only page while a change
/// is pending).
pub(crate) async fn password_form(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    password_page(&s, &staff, StatusCode::OK, None)
}

fn password_page(
    s: &AppState,
    staff: &CurrentStaff,
    status: StatusCode,
    error: Option<String>,
) -> Response {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let title = t("kernel.auth.change_title");
    let flash = error.map(|e| (false, e));
    let body = html! {
        div class="card narrow" {
            h1 { (title) }
            @if staff.must_change_password { p { (t("kernel.auth.must_change")) } }
            (flash_box(&flash))
            form method="post" action=(PASSWORD_PATH) {
                (staff.csrf_field())
                div class="field" {
                    label for="current" { (t("kernel.auth.current_password")) }
                    input id="current" name="current" type="password"
                        autocomplete="current-password" required;
                }
                div class="field" {
                    label for="new" { (t("kernel.auth.new_password")) }
                    input id="new" name="new" type="password" autocomplete="new-password"
                        minlength=(MIN_PASSWORD_CHARS) required;
                    p class="muted" { (t("kernel.auth.password_rule")) }
                }
                div class="field" {
                    label for="confirm" { (t("kernel.auth.confirm_password")) }
                    input id="confirm" name="confirm" type="password" autocomplete="new-password"
                        minlength=(MIN_PASSWORD_CHARS) required;
                }
                button class="wide" type="submit" { (t("kernel.auth.change_submit")) }
            }
        }
    };
    let page = crate::ui::back_office(s, staff, &title, body);
    (status, Html(page.into_string())).into_response()
}

/// `POST /admin/password`.
pub(crate) async fn password_submit(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    headers: HeaderMap,
    Form(f): Form<Fields>,
) -> Response {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let (current, new) = (field(&f, "current"), field(&f, "new"));
    let problem = if new != field(&f, "confirm") {
        Some("kernel.auth.mismatch")
    } else if check_password_rules(new).is_err() {
        Some("kernel.auth.password_rule")
    } else if new == current {
        Some("kernel.auth.same_password")
    } else {
        None
    };
    if let Some(key) = problem {
        return password_page(&s, &staff, StatusCode::BAD_REQUEST, Some(t(key)));
    }
    let token = cookies::get(&headers, SESSION_COOKIE).unwrap_or_default();
    match staff::change_password(&s.services, &staff.id, current, new, &token, &ctx).await {
        Ok(true) => {
            let secure = s.config.staff.cookie_secure;
            let mut resp = (StatusCode::SEE_OTHER, [(header::LOCATION, HOME_PATH)]).into_response();
            let flash = cookies::flash(true, "kernel.auth.password_changed", secure);
            resp.headers_mut().append(header::SET_COOKIE, flash);
            resp
        }
        Ok(false) => {
            let msg = Some(t("kernel.auth.wrong_current"));
            password_page(&s, &staff, StatusCode::BAD_REQUEST, msg)
        }
        Err(e) => error_page(&s, &e),
    }
}
