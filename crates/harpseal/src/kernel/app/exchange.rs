//! `POST /app/session`: the platform token for an App Session.
//!
//! The form carries `channel` (a Channel kind), `token` (the raw LIFF ID
//! token or Telegram `initData`), `next`, and `framed` ("1" when the page
//! runs in an iframe: on a Telegram Web App Channel the cookie is then
//! [`session::Site::Framed`], for web.telegram.org). The Channel's `MiniApp`
//! Capability verifies the token; the account must already exist (it
//! chatted with the bot). Answers:
//! - `Sec-Fetch-Site: cross-site`: 403 (login CSRF; the real exchange is a
//!   same-origin form post);
//! - no such Channel, or no Mini App on it: 404;
//! - on a Channel whose verification is a network call (LIFF: LINE's
//!   verify endpoint), more than 30 exchanges per IP group per minute:
//!   429 before verifying ([`super::limit`]);
//! - a token the platform refuses (`BadSignature`): 403 with a retry
//!   button and at most one Security Event `app.session_refused` per IP
//!   group per minute (Channel kind and IP group, never the token);
//! - a correctly signed but stale or malformed token (`Invalid`, e.g. a
//!   Telegram `initData` older than an hour, which stays the same while
//!   the webview is open): the "open it again from the chat" page, no
//!   retry button, no event;
//! - any other failure: 500, no details; the response carries an
//!   [`ErrorReport`] (`app.exchange` and the error's kind, never its
//!   message) for the runtime's log;
//! - an account the Deployment has never seen: the "not linked" page,
//!   without a session;
//! - otherwise a new session (the account's expired ones deleted in the
//!   same batch), its cookie, and 303 to `next` when it is a safe `/app`
//!   path, else to `/app`.
//!
//! Trade-offs of the limits: Telegram's `initData` is checked locally (one
//! HMAC), so it gets no limit at all; a limit keyed by IP would only lock
//! out Students sharing campus Wi-Fi or carrier NAT. LINE's check costs a
//! request to LINE, so it is capped per IP group (IPv6 /64) at a level a
//! shared NAT should not reach: 30 a minute. Every exchange counts
//! (successful ones too), and one counter row per group per minute keeps
//! the table small; the hourly Job deletes old windows. There is no
//! per-account limit: the account is known only after the verification
//! the limit is meant to spare, and a valid token is never refused for
//! being used again.

use axum::Form;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::sdk::Sdks;
use super::view::{answer, hint, not_linked};
use super::{limit, session};
use crate::audit::RequestCtx;
use crate::port_types::{Statement, Value};
use crate::{AppState, Error, ErrorReport, Result, WebClient};

/// [`ErrorReport::area`] of the exchange.
const AREA: &str = "app.exchange";

/// The exchange form.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ExchangeForm {
    #[serde(default)]
    channel: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    next: String,
    #[serde(default)]
    framed: String,
}

/// `next` when it is `/app` or a path under it made of plain characters,
/// else `/app`. Refuses absolute and protocol-relative URLs, `..`,
/// backslashes, queries and fragments.
pub(crate) fn safe_next(next: &str) -> &str {
    let plain = next
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.'));
    let under_app = next == super::APP_PATH || next.starts_with("/app/");
    if plain && under_app && !next.contains("//") && !next.contains("..") {
        next
    } else {
        super::APP_PATH
    }
}

pub(crate) async fn exchange(
    State(s): State<AppState>,
    ctx: RequestCtx,
    headers: HeaderMap,
    Form(f): Form<ExchangeForm>,
) -> Response {
    let t = match super::texts(&s, &s.config.default_locale).await {
        Ok(t) => t,
        Err(e) => {
            return ErrorReport::attach(StatusCode::SERVICE_UNAVAILABLE.into_response(), AREA, &e);
        }
    };
    // The entry page posts from the same origin; a cross-site post would
    // sign the browser in as someone else (login CSRF).
    if headers
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site")
    {
        return answer(&s, &t, StatusCode::FORBIDDEN, "kernel.app.forbidden", None);
    }
    let next = safe_next(&f.next);
    let d = s.deployment.clone();
    let Some(app) = d.channel(&f.channel).and_then(|c| c.mini_app()) else {
        return answer(&s, &t, StatusCode::NOT_FOUND, "kernel.app.not_found", None);
    };
    // Never shown, and only its kind is logged: the message may echo the
    // token.
    let fail = |e: &Error| {
        let page = answer(
            &s,
            &t,
            StatusCode::INTERNAL_SERVER_ERROR,
            "kernel.app.error",
            Some(next),
        );
        ErrorReport::attach(page, AREA, e)
    };
    let group = ctx.ip.as_deref().map(limit::ip_group);
    if let Some(g) = group.as_deref().filter(|_| networked(&s, &f.channel)) {
        match limit::verify_allowed(&s.services, g).await {
            Ok(true) => {}
            Ok(false) => {
                return answer(
                    &s,
                    &t,
                    StatusCode::TOO_MANY_REQUESTS,
                    "kernel.app.slow_down",
                    Some(next),
                );
            }
            Err(e) => return fail(&e),
        }
    }
    let who = match app.verify_session(&f.token).await {
        Ok(who) => who,
        Err(Error::BadSignature) => {
            if let Err(e) = limit::log_refusal(&s.services, &f.channel, group.as_deref()).await {
                return fail(&e);
            }
            return answer(
                &s,
                &t,
                StatusCode::FORBIDDEN,
                "kernel.app.refused",
                Some(next),
            );
        }
        Err(Error::Invalid(_)) => return hint(&s, &t),
        Err(e) => return fail(&e),
    };
    // The adapter vouches for the external id on its own Channel.
    let account = match account_of(&s, &f.channel, &who.external_id).await {
        Ok(Some(a)) => a,
        Ok(None) => return not_linked(&s, &t, None),
        Err(e) => return fail(&e),
    };
    let started = match session::create(&s.services, &account) {
        Ok((token, stmts)) => s.services.db.batch(stmts).await.map(|_| token),
        Err(e) => Err(e),
    };
    let token = match started {
        Ok(token) => token,
        Err(e) => return fail(&e),
    };
    let mut r = StatusCode::SEE_OTHER.into_response();
    let h = r.headers_mut();
    h.insert(
        header::LOCATION,
        HeaderValue::from_str(next).unwrap_or(HeaderValue::from_static("/app")),
    );
    h.insert(
        header::SET_COOKIE,
        session::cookie(Some(&token), s.config.staff.cookie_secure, site(&s, &f)),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

/// The Channel's verification is a network call: LIFF (LINE's verify
/// endpoint). Telegram and the Fake verify locally.
fn networked(s: &AppState, channel: &str) -> bool {
    matches!(
        Sdks::client_of(&s.deployment, channel),
        Some(WebClient::Liff { .. })
    )
}

/// [`session::Site::Framed`] for a Telegram Web App page in an iframe
/// (Telegram Web); every other page is a top-level webview or tab.
fn site(s: &AppState, f: &ExchangeForm) -> session::Site {
    let telegram = Sdks::client_of(&s.deployment, &f.channel) == Some(WebClient::Telegram);
    if telegram && f.framed == "1" {
        session::Site::Framed
    } else {
        session::Site::Top
    }
}

/// `channel_account.id` of (`channel`, `external_id`).
async fn account_of(s: &AppState, channel: &str, external_id: &str) -> Result<Option<String>> {
    let stmt =
        Statement::new("SELECT id FROM channel_account WHERE channel = ? AND external_id = ?")
            .bind(channel)
            .bind(external_id);
    let rows = s.services.db.query(stmt).await?;
    Ok(rows
        .get(0, "id")
        .and_then(Value::as_text)
        .map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::safe_next;

    #[test]
    fn only_plain_app_paths_survive() {
        for ok in [
            "/app",
            "/app/announcements",
            "/app/announcements/0193-ab_c.d",
        ] {
            assert_eq!(safe_next(ok), ok);
        }
        for bad in [
            "",
            "https://evil.example/app",
            "//evil.example/app",
            "/app//evil.example",
            "/app/../admin",
            "/admin",
            "/apps",
            "/app?x=1",
            "/app/x#y",
            "/app\\evil",
            "/app/%2e%2e",
        ] {
            assert_eq!(safe_next(bad), "/app", "{bad}");
        }
    }
}
