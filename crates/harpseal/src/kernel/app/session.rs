//! `app_session` rows (the App Session) and the `app_session` cookie.
//!
//! - Token: 32 bytes from `Services.random`, hex, only in the cookie;
//!   `app_session.id` is its SHA-256 (as `staff_session`).
//! - One hour from the exchange, no sliding: the platform token behind it
//!   (LIFF ID token, Telegram `initData`) is itself short-lived.
//! - Bound to one Channel Account; the Link and Student are read on every
//!   request, so a revoke or block shows at once.
//! - CSRF: `SHA-256("app-csrf:" + token)`, sent as the `_csrf` field (or
//!   the `X-CSRF-Token` header HTMX adds) on every POST and compared in
//!   constant time. `SameSite=Lax` already stops cross-site POSTs; the
//!   field is defence in depth, as in the back office.

use axum::http::{HeaderMap, HeaderValue};

use crate::auth::sha256_hex;
use crate::message::Locale;
use crate::port_types::{Rows, Statement, Value};
use crate::{Result, Services};

/// The cookie.
pub(crate) const COOKIE: &str = "app_session";
/// How long an App Session lives.
pub(crate) const TTL_MS: i64 = 60 * 60 * 1000;
/// Rows expired longer than this are pruned by the scheduled Job.
pub(crate) const PRUNE_AFTER_MS: i64 = 24 * 60 * 60 * 1000;

/// A linked Student, as the Mini App shows them.
#[derive(Debug, Clone)]
pub(crate) struct LinkedStudent {
    pub(crate) id: String,
    pub(crate) code: String,
    pub(crate) name: String,
}

/// Who is looking at a Mini App page.
#[derive(Debug, Clone)]
pub(crate) struct AppVisitor {
    /// The Channel kind.
    pub(crate) channel: String,
    /// The Student behind the account's active Link, if any. An
    /// `unreachable` or revoked Link, or a Student who `left`, is none.
    pub(crate) student: Option<LinkedStudent>,
    /// The Student's Locale, else the account's platform Locale when the
    /// catalog has it, else the default.
    pub(crate) locale: Locale,
    /// The CSRF token of this session.
    pub(crate) csrf: String,
}

/// The CSRF token bound to an App Session token.
pub(crate) fn csrf_for(token: &str) -> String {
    sha256_hex(format!("app-csrf:{token}").as_bytes())
}

/// A new session for `account`: (cookie token, statements). The batch
/// also deletes the account's expired sessions.
pub(crate) fn create(services: &Services, account: &str) -> Result<(String, Vec<Statement>)> {
    let token = services.random_hex(32)?;
    let now = services.now_ms();
    let stmts = vec![
        Statement::new("DELETE FROM app_session WHERE channel_account_id = ? AND expires_at <= ?")
            .bind(account)
            .bind(now),
        Statement::new(
            "INSERT INTO app_session (id, channel_account_id, created_at, expires_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(sha256_hex(token.as_bytes()))
        .bind(account)
        .bind(now)
        .bind(now + TTL_MS),
    ];
    Ok((token, stmts))
}

/// The newest session of `account` started after `since` exists.
pub(crate) async fn started_since(services: &Services, account: &str, since: i64) -> Result<bool> {
    let stmt = Statement::new(
        "SELECT 1 FROM app_session WHERE channel_account_id = ? AND created_at > ? LIMIT 1",
    )
    .bind(account)
    .bind(since);
    Ok(!services.db.query(stmt).await?.is_empty())
}

/// The visitor behind the request's cookie. A missing, unknown or expired
/// session is `None`; an expired row is deleted on the way.
pub(crate) async fn resolve(
    services: &Services,
    headers: &HeaderMap,
    has_locale: impl Fn(&str) -> bool,
    default_locale: &Locale,
) -> Result<Option<AppVisitor>> {
    let Some(token) = crate::auth::cookie(headers, COOKIE) else {
        return Ok(None);
    };
    let id = sha256_hex(token.as_bytes());
    let stmt = Statement::new(
        "SELECT s.channel_account_id, s.expires_at, ca.channel, ca.platform_locale, \
         st.id AS student_id, st.student_code, st.display_name, st.locale \
         FROM app_session s JOIN channel_account ca ON ca.id = s.channel_account_id \
         LEFT JOIN link l ON l.channel_account_id = ca.id AND l.status = 'active' \
         LEFT JOIN student st ON st.id = l.student_id AND st.status = 'active' \
         WHERE s.id = ? LIMIT 1",
    )
    .bind(id.clone());
    let rows = services.db.query(stmt).await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let expires = rows.get(0, "expires_at").and_then(Value::as_i64);
    if expires.is_none_or(|e| services.now_ms() >= e) {
        let gone = Statement::new("DELETE FROM app_session WHERE id = ?").bind(id);
        services.db.execute(gone).await?;
        return Ok(None);
    }
    let student = text(&rows, "student_id").map(|sid| LinkedStudent {
        id: sid,
        code: text(&rows, "student_code").unwrap_or_default(),
        name: text(&rows, "display_name").unwrap_or_default(),
    });
    let locale = [text(&rows, "locale"), text(&rows, "platform_locale")]
        .into_iter()
        .flatten()
        .find(|l| has_locale(l))
        .map_or_else(|| default_locale.clone(), Locale);
    Ok(Some(AppVisitor {
        channel: text(&rows, "channel").unwrap_or_default(),
        student,
        locale,
        csrf: csrf_for(&token),
    }))
}

fn text(rows: &Rows, col: &str) -> Option<String> {
    rows.get(0, col)
        .and_then(Value::as_text)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// `Set-Cookie` for a new session (or `None` to delete it).
pub(crate) fn cookie(token: Option<&str>, secure: bool) -> HeaderValue {
    let (value, age) = match token {
        Some(t) => (t, TTL_MS / 1000),
        None => ("", 0),
    };
    let secure = if secure { "; Secure" } else { "" };
    let s = format!("{COOKIE}={value}; Path=/app; HttpOnly; SameSite=Lax; Max-Age={age}{secure}");
    HeaderValue::from_str(&s).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// Deletes sessions expired more than [`PRUNE_AFTER_MS`] before `now`;
/// returns how many.
pub(crate) async fn prune(services: &Services, now: i64) -> Result<u64> {
    let stmt =
        Statement::new("DELETE FROM app_session WHERE expires_at < ?").bind(now - PRUNE_AFTER_MS);
    Ok(services.db.execute(stmt).await?.rows_affected)
}
