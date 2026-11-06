//! Revoking a Link: by Staff (or the system) here, by the Student through
//! the chat flow with [`revoke_statements`].

use super::{LinkChange, R, conflict, notify, staff_id, text};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::delivery::ChannelAccountId;
use crate::deployment::Deployment;
use crate::kernel::text::Texts;
use crate::message::{Locale, Message};
use crate::port_types::Statement;
use crate::{Error, Services};

/// Revokes a live (active or unreachable) Link on behalf of Staff (or the
/// system, for the CLI): status `revoked`, `revoked_at`, `revoked_by` (the
/// Staff id; `NULL` for any other actor), audit `link.revoke` with the old
/// status as `before`. The account's Menu is locked and the notice
/// `kernel.link.revoked_by_staff` is Pushed in the Student's Locale.
///
/// The account then waits for a student id with the notice version last
/// accepted **on that account** (Consent kept); when nothing was ever
/// accepted there (a Manual Link whose Card was never accepted, or one
/// that skipped the Card because Consent came from another Channel), it
/// waits on the Consent Card again, so no Consent is ever recorded that
/// nobody accepted from this account.
///
/// `Err(Error::Invalid)` when there is no such Link, `Err(Error::Conflict)`
/// when it is already revoked (including by a concurrent request).
pub async fn revoke_by_staff(
    d: &Deployment,
    link_id: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> R<LinkChange> {
    let s = d.services();
    let default = &d.config().default_locale;
    let link = read_link(s, link_id, default).await?;
    if link.status == "revoked" {
        return Err(already_revoked());
    }
    let (mut stmts, gate) = revoke_statements(s, &link.id, &link.status, actor, ctx)?;
    let accepted_here = "(SELECT notice_version FROM consent WHERE channel_account_id = ? \
        ORDER BY accepted_at DESC LIMIT 1)";
    stmts.push(
        Statement::new(format!(
            "UPDATE channel_account SET flow_state = CASE WHEN {accepted_here} IS NULL \
             THEN 'awaiting_consent' ELSE 'awaiting_student_id' END, \
             flow_data = json_object('consent_version', {accepted_here}), updated_at = ? \
             WHERE id = ? AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)"
        ))
        .bind(link.account.clone())
        .bind(link.account.clone())
        .bind(s.now_ms())
        .bind(link.account.clone())
        .bind(gate),
    );
    let done = s.db.batch(stmts).await?;
    if done.get(1).map(|r| r.rows_affected) != Some(1) {
        return Err(already_revoked());
    }
    let to = ChannelAccountId::new(d.channel_or_err(&link.channel)?.kind(), link.external_id);
    let texts = Texts::load(s, d.catalog_arc(), link.locale.clone()).await?;
    let notice = Message::text(texts.t("kernel.link.revoked_by_staff"));
    let notified = notify(d, &to, None, vec![notice], &link.locale).await;
    Ok(LinkChange {
        link_id: link.id,
        notified,
    })
}

/// The revoke itself, for a caller's batch: the audit row `link.revoke`
/// (inserted only while the Link is live; `before` holds `status`, the
/// status the caller read) and the Link update (run only if that row
/// exists). Returns the statements and the audit row's id, to gate further
/// statements on. The Link update is the second statement.
pub(crate) fn revoke_statements(
    s: &Services,
    link_id: &str,
    status: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> R<(Vec<Statement>, String)> {
    let now = s.now_ms();
    let before = serde_json::json!({ "status": status });
    let after = serde_json::json!({ "status": "revoked" });
    let target = Target::new("link", link_id);
    let live = Statement::new("EXISTS (SELECT 1 FROM link WHERE id = ? AND status <> 'revoked')")
        .bind(link_id);
    let (audit, gate) = audit::entry_if(
        s,
        actor,
        "link.revoke",
        Some(&target),
        Some(before),
        Some(after),
        ctx,
        live,
    )?;
    let update = Statement::new(
        "UPDATE link SET status = 'revoked', revoked_at = ?, revoked_by = ?, updated_at = ? \
         WHERE id = ? AND status <> 'revoked' AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
    )
    .bind(now)
    .bind(staff_id(actor))
    .bind(now)
    .bind(link_id)
    .bind(gate.clone());
    Ok((vec![audit, update], gate))
}

fn already_revoked() -> Error {
    conflict("the Link is already revoked")
}

/// A Link with its account and the Student's Locale.
struct LinkInfo {
    id: String,
    status: String,
    account: String,
    channel: String,
    external_id: String,
    locale: Locale,
}

async fn read_link(s: &Services, link_id: &str, default: &Locale) -> R<LinkInfo> {
    let stmt = Statement::new(
        "SELECT l.id, l.status, l.channel_account_id, ca.channel, ca.external_id, st.locale \
         FROM link l JOIN channel_account ca ON ca.id = l.channel_account_id \
         JOIN student st ON st.id = l.student_id WHERE l.id = ?",
    )
    .bind(link_id);
    let rows = s.db.query(stmt).await?;
    let Some(id) = text(&rows, "id") else {
        return Err(Error::Invalid(format!("no link {link_id}")));
    };
    Ok(LinkInfo {
        id,
        status: text(&rows, "status").unwrap_or_default(),
        account: text(&rows, "channel_account_id").unwrap_or_default(),
        channel: text(&rows, "channel").unwrap_or_default(),
        external_id: text(&rows, "external_id").unwrap_or_default(),
        locale: text(&rows, "locale").map_or_else(|| default.clone(), Locale),
    })
}
