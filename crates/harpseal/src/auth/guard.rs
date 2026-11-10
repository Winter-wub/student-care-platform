//! The back-office guard: session → Staff → Roles → Permissions, forced
//! password change, CSRF on every unsafe method, and the per-request
//! [`RequestCtx`].

use std::sync::Arc;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use super::csrf_body::{Csrf, check_csrf};
use super::{CSRF_FIELD, FLASH_COOKIE, LOGIN_PATH, PASSWORD_PATH, SESSION_COOKIE};
use super::{cookies, csrf_for, roles, session};
use crate::audit::{REQUEST_ID_HEADER, RequestCtx, peer};
use crate::message::Locale;
use crate::{AppState, Result};

/// One back-office page in the navigation.
#[derive(Debug, Clone)]
pub(crate) struct NavItem {
    pub(crate) permission: &'static str,
    /// Full path, `/admin` included.
    pub(crate) path: String,
    pub(crate) title_key: &'static str,
    pub(crate) order: u16,
    pub(crate) group: crate::NavGroup,
}

/// One navigation link the Staff member may open (#123).
#[derive(Debug, Clone)]
pub(crate) struct NavLink {
    pub(crate) label: String,
    pub(crate) href: String,
    pub(crate) group: crate::NavGroup,
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
    /// Largest `multipart/form-data` body the guard reads for `_csrf`.
    pub(crate) body_limit: usize,
    /// Where a body over the limit is sent back to, with an error flash
    /// `(path, catalog key)`: an upload route's own form. Elsewhere the
    /// answer is a 413 page.
    pub(crate) too_large: Option<(&'static str, &'static str)>,
    /// The Module that registered the route, for [`crate::Uploads`] (#74).
    pub(crate) module: Option<&'static str>,
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
    pub(crate) nav: Arc<[NavLink]>,
    /// The request's path, for the navigation's `aria-current` (#123).
    pub(crate) path: String,
    pub(crate) flash: Option<(bool, String)>,
    pub(crate) must_change_password: bool,
    /// The Quota banner for the `#usage-banner` slot (#26), if any.
    pub(crate) usage_banner: Option<Markup>,
    /// This month's usage per Channel, read for the banner (#123).
    pub(crate) usage_month: Option<Arc<Vec<crate::kernel::usage::quota::ChannelMonth>>>,
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
    /// Visible navigation, grouped and sorted.
    pub(crate) fn nav(&self) -> &[NavLink] {
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
/// `/admin` response, and [`APP_FRAME_ANCESTORS`] and `Cache-Control:
/// no-store` to every `/app` one.
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
    let under = |p: &str| parts.uri.path() == p || parts.uri.path().starts_with(&format!("{p}/"));
    let admin = under("/admin");
    let app = under("/app");
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
    if app {
        h.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(APP_FRAME_ANCESTORS),
        );
        // Mini App pages are one Student's data, whatever a Module returns
        // (a fragment, JSON, a file): never cached by anyone (#72).
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    resp
}

/// The Mini App may be framed only by itself and by Telegram Web: LIFF and
/// the Telegram apps load it top-level. Without it, any page (another
/// bot's Mini App inside web.telegram.org shares the partitioned cookie)
/// could frame `/app/announcements/{id}` and steal a tap on "รับทราบ"
/// (clickjacking; the CSRF token does not help, the click is ours).
pub(crate) const APP_FRAME_ANCESTORS: &str = "frame-ancestors 'self' https://web.telegram.org";

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
    let permissions: Arc<[String]> = roles::permissions_of(&state.services, &live.staff_id)
        .await?
        .into();
    let held = |p: &str| permissions.iter().any(|q| q == p);
    let forbidden_permission = gate.permission.filter(|p| !held(p));
    // A path is listed once, under the first Permission held that shows
    // it (the Module settings page has one entry per Permission, #75).
    let mut nav: Vec<NavLink> = Vec::new();
    for n in cx.nav.iter().filter(|n| held(n.permission)) {
        if !nav.iter().any(|l| l.href == n.path) {
            let label = state.catalog.t(&live.locale, n.title_key);
            let (href, group) = (n.path.clone(), n.group);
            nav.push(NavLink { label, href, group });
        }
    }
    let nav: Arc<[NavLink]> = nav.into();
    let flash = cookies::read_flash(req.headers())
        .map(|(ok, key)| (ok, state.catalog.t(&live.locale, &key)));
    let had_flash = cookies::get(req.headers(), FLASH_COOKIE).is_some();
    let usage_view = held(crate::kernel::usage::VIEW);
    // One `usage_daily` read per full-page GET: the banner, and the home's
    // quota card reuses it (#123).
    let usage_month = match renders_a_page(&req) {
        true => crate::kernel::usage::this_month(state).await.map(Arc::new),
        false => None,
    };
    let usage_banner = usage_month
        .as_deref()
        .and_then(|m| crate::kernel::usage::render_banner(state, &live.locale, m, usage_view));
    let staff = CurrentStaff {
        id: live.staff_id,
        email: live.email,
        display_name: live.display_name,
        locale: live.locale,
        must_change_password: live.must_change_password,
        permissions,
        csrf,
        nav,
        path: req.uri().path().to_string(),
        flash,
        usage_banner,
        usage_month,
    };
    // Nothing above read the body. A pending password change and a missing
    // Permission answer without reading it (the handler never runs), so a
    // POST body is buffered, up to the route's limit, only for Staff who
    // may use the route.
    if staff.must_change_password && !gate.during_forced_change {
        return Ok(Err(redirect(req.headers(), PASSWORD_PATH, None)));
    }
    if let Some(p) = forbidden_permission {
        return Ok(Err(forbidden(state, &staff, p)));
    }
    let req = match check_csrf(req, &staff.csrf, gate.body_limit).await {
        (req, Csrf::Ok) => req,
        (_, Csrf::Bad) => {
            let text = state.t("kernel.auth.csrf_failed");
            return Ok(Err(bare_page(state, StatusCode::FORBIDDEN, &text)));
        }
        (req, Csrf::TooLarge) => return Ok(Err(too_large(state, &req, gate))),
    };
    let mut req = req;
    req.extensions_mut().insert(staff);
    if let Some(m) = gate.module {
        req.extensions_mut().insert(crate::module::RouteModule(m));
    }
    Ok(Ok((req, had_flash)))
}

/// Only a full-page GET shows the usage banner: POST redirects, HTMX
/// fragments and `.json`/`.csv` downloads skip its `usage_daily` read.
fn renders_a_page(req: &Request) -> bool {
    let path = req.uri().path();
    *req.method() == Method::GET
        && !req.headers().contains_key("hx-request")
        && !path.ends_with(".json")
        && !path.ends_with(".csv")
}

/// A body over the route's limit: back to the upload form with an error
/// flash, or a 413 page.
fn too_large(state: &AppState, req: &Request, gate: Gate) -> Response {
    match gate.too_large {
        Some((to, key)) => {
            let secure = state.config.staff.cookie_secure;
            let cookie = super::flash_cookie(false, key, secure);
            redirect(req.headers(), to, Some(cookie))
        }
        None => {
            // The problem, then what to do (#140).
            let text = state.t("kernel.auth.too_large");
            let fix = state.t("kernel.auth.too_large_fix");
            let body = html! { div class="card" { p { (text) } p class="fix" { (fix) } } };
            let page = crate::ui::bare(state, &text, body);
            (StatusCode::PAYLOAD_TOO_LARGE, Html(page.into_string())).into_response()
        }
    }
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
pub(crate) fn forbidden(
    state: &crate::PageState,
    staff: &CurrentStaff,
    permission: &str,
) -> Response {
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

fn staff_t(state: &crate::PageState, staff: &CurrentStaff, key: &str) -> String {
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
