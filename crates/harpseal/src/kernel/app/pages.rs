//! The Mini App's GET pages and the acknowledge endpoint. Every handler
//! resolves the App Session first; without one it answers the entry page
//! for its own path (`next`), so a deep link survives the exchange.

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
use crate::{AppState, Result};

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
    session::resolve(&s.services, headers, has, &s.config.default_locale).await
}

/// The entry page for `next`, with the cookie cleared when one was sent.
async fn entry(s: &AppState, headers: &HeaderMap, next: &str) -> Response {
    let t = match super::texts(s, &s.config.default_locale).await {
        Ok(t) => t,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let mut r = view::entry(s, &t, &Sdks::of(&s.deployment), super::safe_next(next));
    if crate::auth::cookie(headers, session::COOKIE).is_some() {
        let clear = session::cookie(None, s.config.staff.cookie_secure);
        r.headers_mut()
            .insert(axum::http::header::SET_COOKIE, clear);
    }
    r
}

fn error(s: &AppState, t: &Texts) -> Response {
    view::answer(
        s,
        t,
        StatusCode::INTERNAL_SERVER_ERROR,
        "kernel.app.error",
        None,
    )
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
async fn open(
    s: &AppState,
    headers: &HeaderMap,
    path: &str,
) -> Result<(AppVisitor, Texts), Box<Response>> {
    let v = match visitor(s, headers).await {
        Ok(Some(v)) => v,
        Ok(None) => return Err(Box::new(entry(s, headers, path).await)),
        Err(_) => return Err(Box::new(StatusCode::SERVICE_UNAVAILABLE.into_response())),
    };
    match super::texts(s, &v.locale).await {
        Ok(t) => Ok((v, t)),
        Err(_) => Err(Box::new(StatusCode::SERVICE_UNAVAILABLE.into_response())),
    }
}

/// `GET /app` and `GET /app/announcements`: Link status and the list.
pub(crate) async fn home(State(s): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
    let (v, t) = match open(&s, &headers, uri.path()).await {
        Ok(x) => x,
        Err(r) => return *r,
    };
    let Some(st) = &v.student else {
        return view::not_linked(&s, &t, Some(&v.csrf));
    };
    let consent = match latest_consent(&s, &st.id).await {
        Ok(c) => c,
        Err(_) => return error(&s, &t),
    };
    let list = if has_news(&s) {
        match news::list_for_student(&s.deployment, &st.id, &v.locale, LIST_LIMIT).await {
            Ok(l) => Some(l),
            Err(_) => return error(&s, &t),
        }
    } else {
        None
    };
    let h = Home {
        consent,
        news: list.as_deref(),
    };
    view::home(&s, &t, &v, &h)
}

async fn latest_consent(s: &AppState, student: &str) -> Result<Option<String>> {
    let stmt = Statement::new(
        "SELECT notice_version FROM consent WHERE student_id = ? \
         ORDER BY accepted_at DESC LIMIT 1",
    )
    .bind(student);
    let rows = s.services.db.query(stmt).await?;
    Ok(rows
        .get(0, "notice_version")
        .and_then(Value::as_text)
        .map(str::to_string))
}

/// `GET /app/announcements/{id}`: one announcement the Student was sent.
pub(crate) async fn detail(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let (v, t) = match open(&s, &headers, &format!("{NEWS_PATH}/{id}")).await {
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
        Err(_) => error(&s, &t),
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
    let (v, t) = match open(&s, &headers, &here).await {
        Ok(x) => x,
        Err(_) if htmx => return hx_redirect(&here),
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
        Err(_) => return error(&s, &t),
    };
    // Nothing to acknowledge: answered like an unknown id.
    if !a.require_ack && !a.retracted {
        return not_found(&s, &t);
    }
    let outcome = if a.retracted {
        AckOutcome::Retracted
    } else {
        match news::ack(&s.deployment, &id, &st.id).await {
            Ok(o) => o,
            Err(_) => return error(&s, &t),
        }
    };
    match outcome {
        AckOutcome::Recorded | AckOutcome::Already if htmx => {
            view::respond(StatusCode::OK, view::acked(&t))
        }
        AckOutcome::Retracted if htmx => view::respond(
            StatusCode::OK,
            maud::html! { p class="app-acked" role="status" { (t.t("kernel.app.retracted")) } },
        ),
        AckOutcome::Recorded | AckOutcome::Already | AckOutcome::Retracted => {
            Redirect::to(&here).into_response()
        }
        _ => not_found(&s, &t),
    }
}

/// HTMX follows `HX-Redirect` with a full page load (the entry page runs).
fn hx_redirect(to: &str) -> Response {
    let mut r = StatusCode::OK.into_response();
    if let Ok(v) = HeaderValue::from_str(to) {
        r.headers_mut().insert("hx-redirect", v);
    }
    r
}
