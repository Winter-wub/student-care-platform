//! `app_session` rows (the App Session) and the `app_session` cookie.
//!
//! - Token: 32 bytes from `Services.random`, hex, only in the cookie;
//!   `app_session.id` is its SHA-256 (as `staff_session`).
//! - One hour from the exchange, no sliding: the platform token behind it
//!   (LIFF ID token, Telegram `initData`) is itself short-lived.
//! - Bound to one Channel Account; the Link and Student are read on every
//!   request, so a revoke or block shows at once.
//! - Bound to one launch (#86): a request that does not come from one of
//!   our own pages drops it before anything is read, and the entry page
//!   asks the platform again who is there ([`super::launch`]). The
//!   exchange also deletes the row of a cookie it is sent; "sign out"
//!   (`POST /app/sign-out`) deletes it on request.
//! - CSRF: `SHA-256("app-csrf:" + token)`, sent as the `_csrf` field (or
//!   the `X-CSRF-Token` header HTMX adds) on every POST and compared in
//!   constant time. `SameSite=Lax` already stops cross-site POSTs; the
//!   field is defence in depth, as in the back office. In a cross-site
//!   iframe (Telegram Web) the cookie is `SameSite=None; Partitioned`
//!   ([`Site::Framed`]) and the field is what stops them.

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
    /// `student.attributes` (an unreadable value is empty).
    pub(crate) attributes: serde_json::Map<String, serde_json::Value>,
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
         st.id AS student_id, st.student_code, st.display_name, st.locale, \
         st.attributes AS student_attributes \
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
        attributes: text(&rows, "student_attributes")
            .and_then(|a| serde_json::from_str(&a).ok())
            .unwrap_or_default(),
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

/// Deletes the session behind the cookie `token` (gone already: nothing).
pub(crate) async fn drop_token(services: &Services, token: &str) -> Result<()> {
    services.db.execute(drop_stmt(token)).await.map(|_| ())
}

/// The statement deleting the session behind the cookie `token`.
pub(crate) fn drop_stmt(token: &str) -> Statement {
    Statement::new("DELETE FROM app_session WHERE id = ?").bind(sha256_hex(token.as_bytes()))
}

fn text(rows: &Rows, col: &str) -> Option<String> {
    rows.get(0, col)
        .and_then(Value::as_text)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// Where the Mini App page runs, for the cookie's `SameSite`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Site {
    /// The chat app's webview or a browser tab: `SameSite=Lax`.
    Top,
    /// A cross-site iframe (Telegram Web, `web.telegram.org`), where a
    /// `Lax` cookie is never sent: `SameSite=None; Secure; Partitioned`
    /// (CHIPS: the cookie is kept per top-level site, so it is not a
    /// tracking cookie). A browser without CHIPS ignores `Partitioned`
    /// and keeps it as an ordinary third-party cookie, or blocks it if it
    /// blocks those; the page then ends at "open from the chat", as it
    /// did with `Lax`. `None` needs `Secure`: with `cookie_secure = false`
    /// (local HTTP) the cookie stays `Lax`.
    Framed,
}

/// `Set-Cookie` for a new session (or `None` to delete it).
pub(crate) fn cookie(token: Option<&str>, secure: bool, site: Site) -> HeaderValue {
    let (value, age) = match token {
        Some(t) => (t, TTL_MS / 1000),
        None => ("", 0),
    };
    let same_site = match site {
        Site::Framed if secure => "None; Secure; Partitioned",
        _ if secure => "Lax; Secure",
        _ => "Lax",
    };
    let s = format!("{COOKIE}={value}; Path=/app; HttpOnly; SameSite={same_site}; Max-Age={age}");
    HeaderValue::from_str(&s).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// `Set-Cookie` headers deleting the cookie in both forms: a partitioned
/// cookie is only deleted by a `Set-Cookie` that is itself `Partitioned`.
/// Without `Secure` there is no partitioned form, so one header.
pub(crate) fn clear(secure: bool) -> Vec<HeaderValue> {
    let mut out = vec![cookie(None, secure, Site::Top)];
    if secure {
        out.push(cookie(None, secure, Site::Framed));
    }
    out
}

/// Deletes at most 500 sessions expired more than [`PRUNE_AFTER_MS`]
/// before `now`; returns how many (index `app_session_expires_idx`,
/// Kernel migration `0008`).
pub(crate) async fn prune(services: &Services, now: i64) -> Result<u64> {
    let stmt = Statement::new(
        "DELETE FROM app_session WHERE id IN \
         (SELECT id FROM app_session WHERE expires_at < ? LIMIT 500)",
    )
    .bind(now - PRUNE_AFTER_MS);
    Ok(services.db.execute(stmt).await?.rows_affected)
}

#[cfg(test)]
mod tests {
    use super::{Site, clear, cookie};

    #[test]
    fn clearing_sends_the_partitioned_form_only_when_it_can_exist() {
        let secure: Vec<_> = clear(true)
            .iter()
            .map(|h| h.to_str().unwrap().to_string())
            .collect();
        assert_eq!(secure.len(), 2);
        assert!(secure[1].contains("SameSite=None; Secure; Partitioned; Max-Age=0"));
        // Without Secure the framed form is Lax too: one header.
        assert_eq!(clear(false).len(), 1);
        let framed = cookie(Some("t"), false, Site::Framed);
        assert!(framed.to_str().unwrap().contains("SameSite=Lax;"));
    }
}
