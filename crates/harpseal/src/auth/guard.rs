//! The back-office guard: session → Staff → Roles → Permissions, forced
//! password change, CSRF on every unsafe method, and the per-request
//! [`RequestCtx`].

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use super::{CSRF_FIELD, CSRF_HEADER, FLASH_COOKIE, LOGIN_PATH, PASSWORD_PATH, SESSION_COOKIE};
use super::{cookies, csrf_for, roles, session};
use crate::audit::{REQUEST_ID_HEADER, RequestCtx, peer};
use crate::message::Locale;
use crate::{AppState, Result};

/// Largest form body the guard reads to find the CSRF field.
const FORM_LIMIT: usize = 1024 * 1024;
/// Largest `multipart/form-data` body read for its `_csrf` part (a roster
/// file of up to 4 MiB plus the form's other fields).
pub(crate) const UPLOAD_LIMIT: usize = 4 * 1024 * 1024 + 64 * 1024;

/// One back-office page in the navigation.
#[derive(Debug, Clone)]
pub(crate) struct NavItem {
    pub(crate) permission: &'static str,
    /// Full path, `/admin` included.
    pub(crate) path: String,
    pub(crate) title_key: &'static str,
    pub(crate) order: u16,
}

/// What every back-office request needs: the page state and the
/// navigation (sorted).
pub(crate) struct AdminCx {
    pub(crate) state: AppState,
    pub(crate) nav: Vec<NavItem>,
}

/// What a guarded route demands.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Gate {
    /// The Permission, or `None` for any signed-in Staff (home, logout).
    pub(crate) permission: Option<&'static str>,
    /// Reachable while a password change is pending.
    pub(crate) during_forced_change: bool,
}

/// The signed-in Staff member, for back-office handlers:
/// `async fn page(staff: CurrentStaff, ..)`. Only guarded routes have one;
/// elsewhere the extractor answers 401.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CurrentStaff {
    /// `staff.id`.
    pub id: String,
    /// Sign-in email.
    pub email: String,
    /// Name shown in the back office.
    pub display_name: String,
    /// The Staff member's Locale.
    pub locale: Locale,
    permissions: Arc<[String]>,
    csrf: String,
    pub(crate) nav: Arc<[(String, String)]>,
    pub(crate) flash: Option<(bool, String)>,
    pub(crate) must_change_password: bool,
}

impl CurrentStaff {
    /// Whether the Staff member holds Permission `id`.
    pub fn can(&self, id: &str) -> bool {
        self.permissions.iter().any(|p| p == id)
    }
    /// The CSRF token every POST must carry.
    pub fn csrf_token(&self) -> &str {
        &self.csrf
    }
    /// The hidden `_csrf` input to put in every `<form method="post">`.
    pub fn csrf_field(&self) -> Markup {
        html! { input type="hidden" name=(CSRF_FIELD) value=(self.csrf); }
    }
    /// Visible navigation: (title, href).
    pub(crate) fn nav(&self) -> &[(String, String)] {
        &self.nav
    }
}

impl<S: Send + Sync> FromRequestParts<S> for CurrentStaff {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<CurrentStaff>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

/// Outermost layer: gives each request a [`RequestCtx`], answers with its
/// id in `x-request-id`, and adds `Cache-Control: no-store`,
/// `X-Frame-Options: DENY` and `Referrer-Policy: same-origin` to every
/// `/admin` response.
pub(crate) async fn request_ctx(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let (mut parts, body) = req.into_parts();
    let id = state.services.new_id().ok();
    let trust = state.config.staff.trust_forwarded_for;
    let ctx = RequestCtx::from_parts(&parts.headers, peer(&parts), trust, id.clone());
    parts.extensions.insert(ctx);
    let admin = parts.uri.path() == "/admin" || parts.uri.path().starts_with("/admin/");
    let mut resp = next.run(Request::from_parts(parts, body)).await;
    let h = resp.headers_mut();
    if let Some(v) = id.and_then(|id| HeaderValue::from_str(&id).ok()) {
        h.insert(REQUEST_ID_HEADER, v);
    }
    if admin {
        // Back-office pages hold personal data: never cached, never framed.
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
        h.insert(
            header::REFERRER_POLICY,
            HeaderValue::from_static("same-origin"),
        );
    }
    resp
}

/// The guard on every back-office route.
pub(crate) async fn guard(
    State((cx, gate)): State<(Arc<AdminCx>, Gate)>,
    req: Request,
    next: Next,
) -> Response {
    match admit(&cx, gate, req).await {
        Ok(Ok((req, had_flash))) => {
            let mut resp = next.run(req).await;
            if had_flash && !resp.status().is_redirection() {
                let secure = cx.state.config.staff.cookie_secure;
                resp.headers_mut()
                    .append(header::SET_COOKIE, cookies::clear(FLASH_COOKIE, secure));
            }
            resp
        }
        Ok(Err(refusal)) => refusal,
        Err(e) => error_page(&cx.state, &e),
    }
}

type Admitted = core::result::Result<(Request, bool), Response>;

async fn admit(cx: &AdminCx, gate: Gate, req: Request) -> Result<Admitted> {
    let state = &cx.state;
    let secure = state.config.staff.cookie_secure;
    let Some(token) = cookies::get(req.headers(), SESSION_COOKIE) else {
        return Ok(Err(to_login(&req, None)));
    };
    let Some(live) = session::resolve(&state.services, &token).await? else {
        return Ok(Err(to_login(
            &req,
            Some(cookies::clear(SESSION_COOKIE, secure)),
        )));
    };
    let csrf = csrf_for(&token);
    let (req, csrf_ok) = check_csrf(req, &csrf).await;
    if !csrf_ok {
        let text = state.t("kernel.auth.csrf_failed");
        return Ok(Err(bare_page(state, StatusCode::FORBIDDEN, &text)));
    }
    if live.must_change_password && !gate.during_forced_change {
        return Ok(Err(redirect(req.headers(), PASSWORD_PATH, None)));
    }
    let permissions: Arc<[String]> = roles::permissions_of(&state.services, &live.staff_id)
        .await?
        .into();
    let held = |p: &str| permissions.iter().any(|q| q == p);
    let forbidden_permission = gate.permission.filter(|p| !held(p));
    let nav: Arc<[(String, String)]> = cx
        .nav
        .iter()
        .filter(|n| held(n.permission))
        .map(|n| (state.catalog.t(&live.locale, n.title_key), n.path.clone()))
        .collect();
    let flash = cookies::read_flash(req.headers())
        .map(|(ok, key)| (ok, state.catalog.t(&live.locale, &key)));
    let had_flash = cookies::get(req.headers(), FLASH_COOKIE).is_some();
    let staff = CurrentStaff {
        id: live.staff_id,
        email: live.email,
        display_name: live.display_name,
        locale: live.locale,
        must_change_password: live.must_change_password,
        permissions,
        csrf,
        nav,
        flash,
    };
    if let Some(p) = forbidden_permission {
        return Ok(Err(forbidden(state, &staff, p)));
    }
    let mut req = req;
    req.extensions_mut().insert(staff);
    Ok(Ok((req, had_flash)))
}

/// Safe methods pass. Otherwise the `X-CSRF-Token` header or the `_csrf`
/// field of a urlencoded body must equal `expected`; the body is read and
/// put back for the handler.
pub(crate) async fn check_csrf(req: Request, expected: &str) -> (Request, bool) {
    if matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return (req, true);
    }
    if let Some(v) = req.headers().get(CSRF_HEADER).and_then(|v| v.to_str().ok()) {
        let ok = cookies::ct_eq(v.as_bytes(), expected.as_bytes());
        return (req, ok);
    }
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    // A file upload (the roster import, #24) carries `_csrf` as a part.
    let boundary = cookies::multipart_boundary(&content_type);
    let is_form = content_type.starts_with("application/x-www-form-urlencoded");
    if !is_form && boundary.is_none() {
        return (req, false);
    }
    let limit = if is_form { FORM_LIMIT } else { UPLOAD_LIMIT };
    let (parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, limit).await else {
        return (Request::from_parts(parts, Body::empty()), false);
    };
    let found = match &boundary {
        Some(b) => cookies::multipart_field(&bytes, b, CSRF_FIELD),
        None => std::str::from_utf8(&bytes)
            .ok()
            .and_then(|b| cookies::form_field(b, CSRF_FIELD)),
    };
    let ok = found.is_some_and(|v| cookies::ct_eq(v.as_bytes(), expected.as_bytes()));
    (Request::from_parts(parts, Body::from(bytes)), ok)
}

/// 303 to `to` (HTMX gets `HX-Redirect` instead, so it swaps the page).
pub(crate) fn redirect(headers: &HeaderMap, to: &str, cookie: Option<HeaderValue>) -> Response {
    let htmx = headers.contains_key("hx-request");
    let mut resp = if htmx {
        (StatusCode::OK, [("hx-redirect", to)]).into_response()
    } else {
        (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response()
    };
    if let Some(c) = cookie {
        resp.headers_mut().append(header::SET_COOKIE, c);
    }
    resp
}

fn to_login(req: &Request, cookie: Option<HeaderValue>) -> Response {
    redirect(req.headers(), LOGIN_PATH, cookie)
}

/// The 403 page, inside the back-office layout.
fn forbidden(state: &AppState, staff: &CurrentStaff, permission: &str) -> Response {
    let title = staff_t(state, staff, "kernel.auth.forbidden_title");
    let body = html! {
        div class="card" {
            h1 { (title) }
            p { (staff_t(state, staff, "kernel.auth.forbidden_body")) }
            p class="muted" { code { (permission) } }
            a class="btn" href="/admin" { (staff_t(state, staff, "kernel.backoffice.home")) }
        }
    };
    let page = crate::ui::back_office(state, staff, &title, body);
    (StatusCode::FORBIDDEN, Html(page.into_string())).into_response()
}

fn staff_t(state: &AppState, staff: &CurrentStaff, key: &str) -> String {
    state.catalog.t(&staff.locale, key)
}

/// A page without navigation (login, CSRF refusal, errors).
pub(crate) fn bare_page(state: &AppState, status: StatusCode, text: &str) -> Response {
    let body = html! { div class="card" { p { (text) } } };
    let page = crate::ui::bare(state, text, body);
    (status, Html(page.into_string())).into_response()
}

/// A 500 page that never shows the error text (it may name a column).
pub(crate) fn error_page(state: &AppState, _e: &crate::Error) -> Response {
    bare_page(
        state,
        StatusCode::INTERNAL_SERVER_ERROR,
        &state.t("kernel.auth.error"),
    )
}
