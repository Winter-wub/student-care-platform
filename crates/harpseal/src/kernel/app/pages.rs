//! The Mini App's GET pages and the acknowledge endpoint. Every handler
//! resolves the App Session first ([`open`]); without one, or on a new
//! launch ([`super::launch`]), it answers the entry page for its own path
//! (`next`), so a deep link survives the exchange.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};

use super::session::{self, AppVisitor};
use super::view::{self, Home};
use super::{NEWS_PATH, sdk::Sdks};
use crate::auth::{ct_eq, form_field};
use crate::kernel::students::is_htmx;
use crate::kernel::text::Texts;
use crate::message::Locale;
use crate::modules::announcements::{self as news, AckOutcome};
use crate::port_types::{Statement, Value};
use crate::{AppState, Error, ErrorReport, Result};

/// Most announcements the page lists.
const LIST_LIMIT: i64 = 100;

/// `GET /`: the LIFF endpoint and the Telegram Web App URL are the
/// Deployment's origin, so the root forwards to `/app`. Only LIFF's
/// `liff.state` (the deep path the SDK redirects to after `liff.init`) is
/// kept, and only when it is a plain `/app` path ([`super::safe_next`]);
/// anything else is dropped, so the SDK can never be sent elsewhere.
pub(crate) async fn root(uri: Uri) -> Redirect {
    let state = uri
        .query()
        .unwrap_or_default()
        .split('&')
        .filter_map(|p| p.split_once('='))
        .find(|(k, _)| *k == "liff.state")
        .and_then(|(_, v)| percent_decode(v));
    match state {
        Some(path) if super::safe_next(&path) == path => {
            let encoded = path.replace('/', "%2F");
            Redirect::to(&format!("{}?liff.state={encoded}", super::APP_PATH))
        }
        _ => Redirect::to(super::APP_PATH),
    }
}

/// One query value, `%XX` and `+` decoded; `None` when malformed.
fn percent_decode(v: &str) -> Option<String> {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                out.push(u8::from_str_radix(v.get(i + 1..i + 3)?, 16).ok()?);
                i += 2;
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

async fn visitor(s: &AppState, headers: &HeaderMap) -> Result<Option<AppVisitor>> {
    let has = |l: &str| s.catalog.has(&Locale(l.to_string()), "kernel.app.title");
    let idle = s.config.mini_app.idle_ms();
    session::resolve(&s.services, headers, idle, has, &s.config.default_locale).await
}

/// The entry page for `next`, with the cookie cleared when one was sent.
async fn entry(s: &AppState, headers: &HeaderMap, next: &str) -> Response {
    let t = match super::texts(s, &s.config.default_locale).await {
        Ok(t) => t,
        Err(e) => return unavailable(&e),
    };
    let mut r = view::entry(s, &t, &Sdks::of(&s.deployment), super::safe_next(next));
    if crate::auth::cookie(headers, session::COOKIE).is_some() {
        for clear in session::clear(s.config.staff.cookie_secure) {
            r.headers_mut()
                .append(axum::http::header::SET_COOKIE, clear);
        }
    }
    r
}

/// [`ErrorReport::area`] of the pages.
const AREA: &str = "app.page";

/// The generic error page, reporting `e`'s kind (never its message) to
/// the runtime's log.
fn error(s: &AppState, t: &Texts, e: &Error) -> Response {
    let page = view::answer(
        s,
        t,
        StatusCode::INTERNAL_SERVER_ERROR,
        "kernel.app.error",
        None,
    );
    ErrorReport::attach(page, AREA, e)
}

/// 503 when not even the texts or the session can be read.
fn unavailable(e: &Error) -> Response {
    ErrorReport::attach(StatusCode::SERVICE_UNAVAILABLE.into_response(), AREA, e)
}

fn not_found(s: &AppState, t: &Texts) -> Response {
    view::answer(s, t, StatusCode::NOT_FOUND, "kernel.app.not_found", None)
}

/// The Deployment runs the Announcements Module (its tables exist).
fn has_news(s: &AppState) -> bool {
    s.deployment
        .migrations()
        .iter()
        .any(|m| m.module == news::NAME)
}

/// A visitor and their texts, or the response to send instead.
pub(super) async fn open(
    s: &AppState,
    headers: &HeaderMap,
    path: &str,
    query: Option<&str>,
) -> Result<(AppVisitor, Texts), Box<Response>> {
    // A new launch (#86): whoever opened the page proves it again before
    // the session's Student shows. `?launch` is the page script asking for
    // one (another tab's session, a page restored from the bfcache).
    if let Some(token) = crate::auth::cookie(headers, session::COOKIE)
        && (!super::launch::continues(headers) || super::launch::forced(query))
    {
        if let Err(e) = session::drop_token(&s.services, &token).await {
            return Err(Box::new(unavailable(&e)));
        }
        return Err(Box::new(entry(s, headers, path).await));
    }
    let v = match visitor(s, headers).await {
        Ok(Some(v)) => v,
        Ok(None) => return Err(Box::new(entry(s, headers, path).await)),
        Err(e) => return Err(Box::new(unavailable(&e))),
    };
    match super::texts(s, &v.locale).await {
        Ok(t) => Ok((v, t)),
        Err(e) => Err(Box::new(unavailable(&e))),
    }
}

/// `GET /app` and `GET /app/announcements`: Link status and the list.
pub(crate) async fn home(State(s): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
    let (v, t) = match open(&s, &headers, uri.path(), uri.query()).await {
        Ok(x) => x,
        Err(r) => return *r,
    };
    let Some(st) = &v.student else {
        return view::not_linked(&s, &t, Some(&v.csrf));
    };
    let current = t.consent_version(&s.config.consent.version);
    let (consent, reconsent) = match latest_consent(&s, &st.id, &current).await {
        Ok(c) => c,
        Err(e) => return error(&s, &t, &e),
    };
    let list = if has_news(&s) {
        match news::list_for_student(&s.deployment, &st.id, &v.locale, LIST_LIMIT).await {
            Ok(l) => Some(l),
            Err(e) => return error(&s, &t, &e),
        }
    } else {
        None
    };
    let links: Vec<_> = s
        .deployment
        .app_links()
        .iter()
        .map(|l| (t.t(l.label_key), l.path))
        .collect();
    let h = Home {
        consent,
        reconsent,
        links: &links,
        news: list.as_deref(),
    };
    view::home(&s, &t, &v, &h)
}

/// The latest Consent's notice version, and whether a re-consent is
/// pending: the Student has a Consent but none for `current` (the rule
/// the chat flow applies, `kernel::flow::linked`).
async fn latest_consent(
    s: &AppState,
    student: &str,
    current: &str,
) -> Result<(Option<String>, bool)> {
    let stmt = Statement::new(
        "SELECT notice_version, EXISTS (SELECT 1 FROM consent WHERE student_id = ? \
         AND notice_version = ?) AS has_current FROM consent WHERE student_id = ? \
         ORDER BY accepted_at DESC LIMIT 1",
    )
    .bind(student)
    .bind(current)
    .bind(student);
    let rows = s.services.db.query(stmt).await?;
    let latest = rows
        .get(0, "notice_version")
        .and_then(Value::as_text)
        .map(str::to_string);
    let has_current = rows.get(0, "has_current").and_then(Value::as_i64) == Some(1);
    let pending = latest.is_some() && !has_current;
    Ok((latest, pending))
}

/// `GET /app/announcements/{id}`: one announcement the Student was sent.
pub(crate) async fn detail(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    uri: Uri,
) -> Response {
    let (v, t) = match open(&s, &headers, &format!("{NEWS_PATH}/{id}"), uri.query()).await {
        Ok(x) => x,
        Err(r) => return *r,
    };
    let Some(st) = &v.student else {
        return view::not_linked(&s, &t, Some(&v.csrf));
    };
    if !has_news(&s) {
        return not_found(&s, &t);
    }
    match news::detail_for_student(&s.deployment, &id, &st.id, &v.locale).await {
        Ok(Some(a)) => view::detail(&s, &t, &v, &a),
        Ok(None) => not_found(&s, &t),
        Err(e) => error(&s, &t, &e),
    }
}

/// `POST /app/announcements/{id}/ack`: the one-way "รับทราบ". The Student's
/// delivery row is checked first (none: 404, whether the announcement
/// exists or not), then [`news::ack`]. HTMX gets the line that replaces
/// the button; a plain form post goes back to the detail page.
pub(crate) async fn ack(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: String,
) -> Response {
    let here = format!("{NEWS_PATH}/{id}");
    let htmx = is_htmx(&headers);
    // A real ack is same-origin, even in Telegram Web's iframe; with the
    // partitioned `SameSite=None` cookie this is defence in depth next to
    // the CSRF token.
    if headers
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site")
    {
        return match super::texts(&s, &s.config.default_locale).await {
            Ok(t) => view::answer(&s, &t, StatusCode::FORBIDDEN, "kernel.app.forbidden", None),
            Err(e) => unavailable(&e),
        };
    }
    let (v, t) = match open(&s, &headers, &here, None).await {
        Ok(x) => x,
        // HTMX does not swap an error status: a full reload of the detail
        // page shows the entry page or the error, and the report travels.
        Err(r) if htmx => return hx_redirect_from(&here, &r),
        Err(r) => return *r,
    };
    let sent = headers
        .get(crate::auth::CSRF_HEADER)
        .and_then(|h| h.to_str().ok())
        .or_else(|| form_field(&body, crate::auth::CSRF_FIELD))
        .unwrap_or_default();
    if !ct_eq(sent.as_bytes(), v.csrf.as_bytes()) {
        return view::answer(&s, &t, StatusCode::FORBIDDEN, "kernel.app.forbidden", None);
    }
    let Some(st) = &v.student else {
        return view::not_linked(&s, &t, Some(&v.csrf));
    };
    if !has_news(&s) {
        return not_found(&s, &t);
    }
    let a = match news::detail_for_student(&s.deployment, &id, &st.id, &v.locale).await {
        Ok(Some(a)) => a,
        Ok(None) => return not_found(&s, &t),
        Err(e) => return error(&s, &t, &e),
    };
    // Nothing to acknowledge: answered like an unknown id.
    if !a.require_ack && !a.unavailable() {
        return not_found(&s, &t);
    }
    // Retracted or expired (#77): nothing recorded, the line says why.
    let outcome = if a.retracted {
        AckOutcome::Retracted
    } else if a.expired {
        AckOutcome::Expired
    } else {
        match news::ack(&s.deployment, &id, &st.id).await {
            Ok(o) => o,
            Err(e) => return error(&s, &t, &e),
        }
    };
    match outcome {
        AckOutcome::Recorded | AckOutcome::Already if htmx => {
            view::respond(StatusCode::OK, view::acked(&t))
        }
        AckOutcome::Retracted | AckOutcome::Expired if htmx => {
            let key = view::gone_key(&a).unwrap_or("kernel.app.retracted");
            view::respond(
                StatusCode::OK,
                maud::html! { p class="app-acked" role="status" { (t.t(key)) } },
            )
        }
        AckOutcome::Recorded
        | AckOutcome::Already
        | AckOutcome::Retracted
        | AckOutcome::Expired => Redirect::to(&here).into_response(),
        _ => not_found(&s, &t),
    }
}

/// [`hx_redirect`] carrying `r`'s [`ErrorReport`], if any.
pub(super) fn hx_redirect_from(to: &str, r: &Response) -> Response {
    let mut out = hx_redirect(to);
    if let Some(report) = ErrorReport::of(r).copied() {
        out.extensions_mut().insert(report);
    }
    out
}

/// HTMX follows `HX-Redirect` with a full page load (the entry page runs).
pub(super) fn hx_redirect(to: &str) -> Response {
    let mut r = StatusCode::OK.into_response();
    if let Ok(v) = HeaderValue::from_str(to) {
        r.headers_mut().insert("hx-redirect", v);
    }
    r
}
