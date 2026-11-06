//! Revoking a Link: by Staff (or the system) here, by the Student through
//! the chat flow with [`revoke_statements`].

use super::{LinkChange, R, conflict, notify, staff_id, text};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::delivery::ChannelAccountId;
use crate::deployment::Deployment;
use crate::kernel::text::Texts;
use crate::message::{Locale, Message};
use crate::port_types::{Rows, Statement, Value};
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
    let stmts = staff_revoke_statements(s, &link, actor, ctx, None)?;
    let done = s.db.batch(stmts).await?;
    if done.get(1).map(|r| r.rows_affected) != Some(1) {
        return Err(already_revoked());
    }
    let link_id = link.id.clone();
    let notified = notify_revoked(d, link).await?;
    Ok(LinkChange { link_id, notified })
}

/// The three statements of a Staff (or system) revoke of `link`, for a
/// caller's batch: the gated audit row and Link update of
/// [`revoke_statements`] (the update is the second statement), then the
/// account's flow state (see [`revoke_by_staff`]). With `gate` (an audit
/// row id written earlier in the same batch), nothing happens unless that
/// row exists: the Students page's "set left" revokes every Link only if
/// its own status change went through.
pub(crate) fn staff_revoke_statements(
    s: &Services,
    link: &LinkInfo,
    actor: &Actor,
    ctx: &RequestCtx,
    gate: Option<&str>,
) -> R<Vec<Statement>> {
    let (mut stmts, own) = gated_revoke_statements(s, &link.id, &link.status, actor, ctx, gate)?;
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
        .bind(own),
    );
    Ok(stmts)
}

/// After a committed revoke: Pushes `kernel.link.revoked_by_staff` in the
/// Student's Locale through [`notify`], which is also what locks the
/// account's Menu (it is called with no Menu). `true` when the Student was
/// told; `Err` when the Link's Channel is not registered (any more) or the
/// texts cannot be read.
pub(crate) async fn notify_revoked(d: &Deployment, link: LinkInfo) -> R<bool> {
    let s = d.services();
    let to = ChannelAccountId::new(d.channel_or_err(&link.channel)?.kind(), link.external_id);
    let texts = Texts::load(s, d.catalog_arc(), link.locale.clone()).await?;
    let notice = Message::text(texts.t("kernel.link.revoked_by_staff"));
    Ok(notify(d, &to, None, vec![notice], &link.locale).await)
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
    gated_revoke_statements(s, link_id, status, actor, ctx, None)
}

/// [`revoke_statements`], run only if the audit row `gate` exists.
fn gated_revoke_statements(
    s: &Services,
    link_id: &str,
    status: &str,
    actor: &Actor,
    ctx: &RequestCtx,
    gate: Option<&str>,
) -> R<(Vec<Statement>, String)> {
    let now = s.now_ms();
    let before = serde_json::json!({ "status": status });
    let after = serde_json::json!({ "status": "revoked" });
    let target = Target::new("link", link_id);
    let live = match gate {
        None => Statement::new("EXISTS (SELECT 1 FROM link WHERE id = ? AND status <> 'revoked')")
            .bind(link_id),
        Some(g) => Statement::new(
            "EXISTS (SELECT 1 FROM link WHERE id = ? AND status <> 'revoked') \
             AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
        )
        .bind(link_id)
        .bind(g),
    };
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
pub(crate) struct LinkInfo {
    pub(crate) id: String,
    status: String,
    account: String,
    channel: String,
    external_id: String,
    locale: Locale,
}

const LINK_INFO: &str = "SELECT l.id, l.status, l.channel_account_id, ca.channel, \
    ca.external_id, st.locale FROM link l JOIN channel_account ca ON ca.id = l.channel_account_id \
    JOIN student st ON st.id = l.student_id";

/// The live (active or unreachable) Links of a Student.
pub(crate) async fn live_links_of(
    s: &Services,
    student_id: &str,
    default: &Locale,
) -> R<Vec<LinkInfo>> {
    let stmt = Statement::new(format!(
        "{LINK_INFO} WHERE l.student_id = ? AND l.status <> 'revoked' ORDER BY l.id"
    ))
    .bind(student_id);
    let rows = s.db.query(stmt).await?;
    Ok((0..rows.len())
        .map(|i| info_at(&rows, i, default))
        .collect())
}

/// The live Links of several Students (the roster import's left batch),
/// as `(student id, Link)`, in at most 50 ids per query.
pub(crate) async fn live_links_of_students(
    s: &Services,
    student_ids: &[String],
    default: &Locale,
) -> R<Vec<(String, LinkInfo)>> {
    let mut out = Vec::new();
    for group in student_ids.chunks(50) {
        let marks = vec!["?"; group.len()].join(", ");
        let sql = format!(
            "{} WHERE l.student_id IN ({marks}) AND l.status <> 'revoked' ORDER BY l.id",
            LINK_INFO.replacen("SELECT ", "SELECT l.student_id, ", 1)
        );
        let stmt = group
            .iter()
            .fold(Statement::new(sql), |st, id| st.bind(id.as_str()));
        let rows = s.db.query(stmt).await?;
        for i in 0..rows.len() {
            let student = rows
                .get(i, "student_id")
                .and_then(Value::as_text)
                .unwrap_or_default()
                .to_string();
            out.push((student, info_at(&rows, i, default)));
        }
    }
    Ok(out)
}

fn info_at(rows: &Rows, i: usize, default: &Locale) -> LinkInfo {
    let t = |c: &str| {
        rows.get(i, c)
            .and_then(Value::as_text)
            .unwrap_or_default()
            .to_string()
    };
    LinkInfo {
        id: t("id"),
        status: t("status"),
        account: t("channel_account_id"),
        channel: t("channel"),
        external_id: t("external_id"),
        locale: rows
            .get(i, "locale")
            .and_then(Value::as_text)
            .map_or_else(|| default.clone(), |l| Locale(l.to_string())),
    }
}

async fn read_link(s: &Services, link_id: &str, default: &Locale) -> R<LinkInfo> {
    let stmt = Statement::new(format!("{LINK_INFO} WHERE l.id = ?")).bind(link_id);
    let rows = s.db.query(stmt).await?;
    if text(&rows, "id").is_none() {
        return Err(Error::Invalid(format!("no link {link_id}")));
    }
    Ok(info_at(&rows, 0, default))
}
