//! `POST /app/sign-out` (#86): the Student ends their App Session from
//! the Mini App (the button on the home page), e.g. before leaving a
//! shared computer.
//!
//! Same rules as every Mini App post: `Sec-Fetch-Site: cross-site` is 403,
//! and with a live session the `_csrf` field (or `X-CSRF-Token`) must be
//! its token, else 403 and the session stays. Then the row is deleted, the
//! cookie cleared in both forms ([`session::clear`]) and the "signed out"
//! page shown, with no button: the next launch from the chat signs in
//! again. Inside Telegram or the LINE app the page then closes itself. Without a live session (expired, or signed out already) the
//! cookie is cleared and the same page shown.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use maud::{PreEscaped, html};

use super::sdk::Sdks;
use super::session;
use super::view::{self, CLEAR_GUARD_JS};
use crate::auth::{CSRF_FIELD, CSRF_HEADER, ct_eq, form_field};
use crate::message::Locale;
use crate::{AppState, ErrorReport};

/// After a moment to read the text, closes the Mini App where the platform
/// can: Telegram (`WebApp.close()`, inside Telegram only) and LIFF in the
/// LINE app (`liff.closeWindow()`). In a browser the page stays.
const CLOSE_JS: &str = r#"(function () {
  var d = document.getElementById('bye').dataset;
  setTimeout(function () {
    var tg = window.Telegram && window.Telegram.WebApp;
    if (tg && tg.initData) { tg.close(); return; }
    var lf = window.liff;
    if (lf && d.liff) {
      lf.init({ liffId: d.liff }).then(function () {
        if (lf.isInClient()) { lf.closeWindow(); }
      }, function () {});
    }
  }, 1500);
})();"#;

/// The route.
pub(crate) const SIGN_OUT_PATH: &str = "/app/sign-out";

/// [`ErrorReport::area`] of the sign-out.
const AREA: &str = "app.sign_out";

pub(crate) async fn sign_out(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let has = |l: &str| s.catalog.has(&Locale(l.to_string()), "kernel.app.title");
    let resolved = session::resolve(
        &s.services,
        &headers,
        s.config.mini_app.idle_ms(),
        has,
        &s.config.default_locale,
    )
    .await;
    let visitor = match resolved {
        Ok(v) => v,
        Err(e) => return unavailable(&e),
    };
    let locale = visitor
        .as_ref()
        .map_or(&s.config.default_locale, |v| &v.locale);
    let t = match super::texts(&s, locale).await {
        Ok(t) => t,
        Err(e) => return unavailable(&e),
    };
    let cross_site = headers
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site");
    let sent = headers
        .get(CSRF_HEADER)
        .and_then(|h| h.to_str().ok())
        .or_else(|| form_field(&body, CSRF_FIELD))
        .unwrap_or_default();
    let forged = visitor
        .as_ref()
        .is_some_and(|v| !ct_eq(sent.as_bytes(), v.csrf.as_bytes()));
    if cross_site || forged {
        return view::answer(&s, &t, StatusCode::FORBIDDEN, "kernel.app.forbidden", None);
    }
    if let Some(token) = crate::auth::cookie(&headers, session::COOKIE)
        && let Err(e) = session::drop_token(&s.services, &token).await
    {
        let status = StatusCode::INTERNAL_SERVER_ERROR;
        let page = view::answer(&s, &t, status, "kernel.app.error", None);
        return ErrorReport::attach(page, AREA, &e);
    }
    let text = t.t("kernel.app.signed_out");
    let sdks = Sdks::of(&s.deployment);
    let liff = sdks.liff.as_ref().map(|(_, id)| id.as_str());
    let body = html! {
        div class="card app-center" id="bye" data-liff=[liff] { p { (text) } }
        script { (PreEscaped(CLEAR_GUARD_JS)) }
        @if sdks.telegram.is_some() { script src=(view::TELEGRAM_SDK) {} }
        @if liff.is_some() { script src=(view::LIFF_SDK) charset="utf-8" {} }
        script { (PreEscaped(CLOSE_JS)) }
    };
    let notice = t.t("kernel.app.notice_link");
    let doc = crate::ui::mini_app_session(&s, &t.t("kernel.app.sign_out"), None, &notice, body);
    let mut r = view::respond(StatusCode::OK, doc);
    for clear in session::clear(s.config.staff.cookie_secure) {
        r.headers_mut().append(header::SET_COOKIE, clear);
    }
    r
}

fn unavailable(e: &crate::Error) -> Response {
    ErrorReport::attach(StatusCode::SERVICE_UNAVAILABLE.into_response(), AREA, e)
}
