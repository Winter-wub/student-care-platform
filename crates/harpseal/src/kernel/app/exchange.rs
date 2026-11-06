//! `POST /app/session`: the platform token for an App Session.
//!
//! The form carries `channel` (a Channel kind), `token` (the raw LIFF ID
//! token or Telegram `initData`) and `next`. The Channel's `MiniApp`
//! Capability verifies the token; the account must already exist (it
//! chatted with the bot). Answers:
//! - no such Channel, or no Mini App on it: 404;
//! - a token the platform refuses (`BadSignature` / `Invalid`): 403 with a
//!   retry button and a Security Event `app.session_refused` (Channel kind
//!   and client IP, never the token);
//! - any other failure: 500, no details;
//! - ten refusals from one IP within a minute: 429 before verifying;
//! - a second session for the same account within one second: 429;
//! - an account the Deployment has never seen: the "not linked" page,
//!   without a session;
//! - otherwise a new session (the account's expired ones deleted in the
//!   same batch), its cookie, and 303 to `next` when it is a safe `/app`
//!   path, else to `/app`.

use axum::Form;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::session;
use super::view::{answer, not_linked};
use crate::audit::RequestCtx;
use crate::port_types::{Statement, Value};
use crate::{AppState, Error, Result};

/// Refusals one IP may collect within [`REFUSAL_WINDOW_MS`].
const REFUSALS_PER_IP: i64 = 10;
const REFUSAL_WINDOW_MS: i64 = 60_000;
/// One session per account per this long.
const PER_ACCOUNT_MS: i64 = 1_000;
/// Security Event kind of a refused token.
pub(crate) const REFUSED: &str = "app.session_refused";

/// The exchange form.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ExchangeForm {
    #[serde(default)]
    channel: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    next: String,
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
    Form(f): Form<ExchangeForm>,
) -> Response {
    let Ok(t) = super::texts(&s, &s.config.default_locale).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let next = safe_next(&f.next);
    let d = s.deployment.clone();
    let Some(app) = d.channel(&f.channel).and_then(|c| c.mini_app()) else {
        return answer(&s, &t, StatusCode::NOT_FOUND, "kernel.app.not_found", None);
    };
    let fail = |e: &Error| {
        let _ = e; // never shown or logged: it may echo the token
        answer(
            &s,
            &t,
            StatusCode::INTERNAL_SERVER_ERROR,
            "kernel.app.error",
            Some(next),
        )
    };
    match refused_lately(&s, ctx.ip.as_deref()).await {
        Ok(true) => {
            return answer(
                &s,
                &t,
                StatusCode::TOO_MANY_REQUESTS,
                "kernel.app.slow_down",
                Some(next),
            );
        }
        Ok(false) => {}
        Err(e) => return fail(&e),
    }
    let who = match app.verify_session(&f.token).await {
        Ok(who) => who,
        Err(Error::BadSignature | Error::Invalid(_)) => {
            let mut details = vec![("channel", f.channel.clone())];
            if let Some(ip) = &ctx.ip {
                details.push(("ip", ip.clone()));
            }
            let logged =
                crate::kernel::security::record(&s.services, REFUSED, None, None, &details).await;
            if let Err(e) = logged {
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
        Err(e) => return fail(&e),
    };
    // The adapter vouches for the external id on its own Channel.
    let account = match account_of(&s, &f.channel, &who.external_id).await {
        Ok(Some(a)) => a,
        Ok(None) => return not_linked(&s, &t, None),
        Err(e) => return fail(&e),
    };
    let since = s.services.now_ms() - PER_ACCOUNT_MS;
    match session::started_since(&s.services, &account, since).await {
        Ok(true) => {
            return answer(
                &s,
                &t,
                StatusCode::TOO_MANY_REQUESTS,
                "kernel.app.slow_down",
                Some(next),
            );
        }
        Ok(false) => {}
        Err(e) => return fail(&e),
    }
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
        session::cookie(Some(&token), s.config.staff.cookie_secure),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

/// The client at `ip` collected [`REFUSALS_PER_IP`] refusals lately.
async fn refused_lately(s: &AppState, ip: Option<&str>) -> Result<bool> {
    let Some(ip) = ip else { return Ok(false) };
    let stmt = Statement::new(
        "SELECT count(*) AS n FROM security_event WHERE at > ? AND kind = ? \
         AND json_extract(details, '$.ip') = ?",
    )
    .bind(s.services.now_ms() - REFUSAL_WINDOW_MS)
    .bind(REFUSED)
    .bind(ip);
    let rows = s.services.db.query(stmt).await?;
    Ok(rows.get(0, "n").and_then(Value::as_i64).unwrap_or(0) >= REFUSALS_PER_IP)
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
