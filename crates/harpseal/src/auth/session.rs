//! `staff_session` rows: created at login, checked (and slid) on every
//! request, deleted at logout or expiry.

use super::{ABSOLUTE_MS, SLIDING_MS, TOUCH_EVERY_MS, sha256_hex};
use crate::audit::RequestCtx;
use crate::message::Locale;
use crate::port_types::{Rows, Statement};
use crate::{Result, Services};

/// The Staff member behind a live session.
#[derive(Debug, Clone)]
pub(crate) struct Live {
    pub(crate) staff_id: String,
    pub(crate) email: String,
    pub(crate) display_name: String,
    pub(crate) locale: Locale,
    pub(crate) must_change_password: bool,
}

/// `staff_session.id` for a cookie token.
pub(crate) fn id_of(token: &str) -> String {
    sha256_hex(token.as_bytes())
}

/// Starts a session; returns the cookie token (shown only to the browser).
pub(crate) async fn create(
    services: &Services,
    staff_id: &str,
    ctx: &RequestCtx,
) -> Result<String> {
    let (token, stmt) = insert(services, staff_id, ctx)?;
    services.db.execute(stmt).await?;
    Ok(token)
}

/// A new session's cookie token and its `INSERT`, for a batch.
pub(crate) fn insert(
    services: &Services,
    staff_id: &str,
    ctx: &RequestCtx,
) -> Result<(String, Statement)> {
    let token = services.random_hex(32)?;
    let now = services.now_ms();
    let stmt = Statement::new(
        "INSERT INTO staff_session (id, staff_id, created_at, expires_at, last_seen_at, ip, \
         user_agent) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id_of(&token))
    .bind(staff_id)
    .bind(now)
    .bind(now + ABSOLUTE_MS)
    .bind(now)
    .bind(ctx.ip.clone())
    .bind(ctx.user_agent.clone());
    Ok((token, stmt))
}

/// Deletes sessions past either limit at `now`; returns how many.
pub(crate) async fn prune(services: &Services, now: i64) -> Result<u64> {
    let stmt =
        Statement::new("DELETE FROM staff_session WHERE expires_at <= ? OR last_seen_at <= ?")
            .bind(now)
            .bind(now - SLIDING_MS);
    Ok(services.db.execute(stmt).await?.rows_affected)
}

/// The live session for `token`, sliding it forward. An expired session,
/// or one whose Staff member is disabled, is deleted and gives `None`.
pub(crate) async fn resolve(services: &Services, token: &str) -> Result<Option<Live>> {
    let id = id_of(token);
    let rows = services
        .db
        .query(
            Statement::new(
                "SELECT s.staff_id, s.expires_at, s.last_seen_at, st.email, st.display_name, \
                 st.locale, st.status, st.must_change_password \
                 FROM staff_session s JOIN staff st ON st.id = s.staff_id WHERE s.id = ?",
            )
            .bind(id.clone()),
        )
        .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let int = |c: &str| rows.get(0, c).and_then(|v| v.as_i64()).unwrap_or(0);
    let now = services.now_ms();
    let expired = now >= int("expires_at") || now >= int("last_seen_at") + SLIDING_MS;
    if expired || text(&rows, "status") != "active" {
        delete_id(services, &id).await?;
        return Ok(None);
    }
    if now - int("last_seen_at") >= TOUCH_EVERY_MS {
        let touch = Statement::new("UPDATE staff_session SET last_seen_at = ? WHERE id = ?")
            .bind(now)
            .bind(id);
        services.db.execute(touch).await?;
    }
    Ok(Some(Live {
        staff_id: text(&rows, "staff_id"),
        email: text(&rows, "email"),
        display_name: text(&rows, "display_name"),
        locale: Locale(text(&rows, "locale")),
        must_change_password: int("must_change_password") == 1,
    }))
}

/// Ends the session for `token`.
pub(crate) async fn delete(services: &Services, token: &str) -> Result<()> {
    delete_id(services, &id_of(token)).await
}

async fn delete_id(services: &Services, id: &str) -> Result<()> {
    let stmt = Statement::new("DELETE FROM staff_session WHERE id = ?").bind(id);
    services.db.execute(stmt).await.map(|_| ())
}

/// Ends every session of `staff_id` except `keep` (a token), for a batch.
pub(crate) fn delete_others(staff_id: &str, keep: Option<&str>) -> Statement {
    Statement::new("DELETE FROM staff_session WHERE staff_id = ? AND id <> ?")
        .bind(staff_id)
        .bind(keep.map(id_of).unwrap_or_default())
}

pub(crate) fn text(rows: &Rows, column: &str) -> String {
    rows.get(0, column)
        .and_then(|v| v.as_text())
        .unwrap_or_default()
        .to_string()
}
