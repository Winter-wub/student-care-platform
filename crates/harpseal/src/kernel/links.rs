//! The Link service: every change to a Link after Verification goes
//! through here, so the audit row and the one-live-Link invariant live in
//! one place. The back office (Manual Link / revoke on a Student's page)
//! and the CLI call [`manual_link`] and [`revoke_by_staff`]; the chat
//! flow's self-unlink uses the same revoke statements with the Student as
//! the actor.
//!
//! Every write is one `Db::batch`. A revoke is gated on its own audit row:
//! the row is inserted only while the Link is live, and the rest of the
//! batch runs only if that row exists, so of two racing revokes exactly
//! one changes anything and writes one audit row. A Manual Link relies on
//! the partial unique indexes on `link` (one live Link per Student per
//! Channel, and per Channel Account): a lost race fails the whole batch
//! and comes back as [`Error::Conflict`].
//!
//! The Pushes go out after the write. A failed Push does not undo it:
//! [`LinkChange::notified`] tells the caller whether the Student was told.

use crate::audit::{self, Actor, RequestCtx, Target};
use crate::delivery::ChannelAccountId;
use crate::deployment::Deployment;
use crate::kernel::flow;
use crate::kernel::text::Texts;
use crate::message::{Locale, Menu, Message};
use crate::port_types::{Statement, Value};
use crate::{Error, Result, Services};

/// What a Link change did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LinkChange {
    /// `link.id` of the Link created or revoked.
    pub link_id: String,
    /// The notice (and, for a Manual Link, the Consent Card) reached the
    /// Student's account. `false` when the account is blocked or the
    /// Channel refused the send; the change itself stands.
    pub notified: bool,
}

/// Revokes a live (active or unreachable) Link on behalf of Staff (or the
/// system, for the CLI): status `revoked`, `revoked_at`, `revoked_by` (the
/// Staff id; `NULL` for [`Actor::System`]), audit `link.revoke`. The
/// account goes back to waiting for a student id (Consent kept), its Menu
/// is locked and the notice `kernel.link.revoked_by_staff` is Pushed in
/// the Student's Locale.
///
/// `Err(Error::Invalid)` when there is no such Link, `Err(Error::Conflict)`
/// when it is already revoked (including by a concurrent request).
pub async fn revoke_by_staff(
    d: &Deployment,
    link_id: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<LinkChange> {
    let s = d.services();
    let link = read_link(s, link_id).await?;
    if link.status == "revoked" {
        return Err(already_revoked());
    }
    let (mut stmts, gate) = revoke_statements(s, &link.id, &link.student, actor, ctx)?;
    stmts.push(
        Statement::new(
            "UPDATE channel_account SET flow_state = 'awaiting_student_id', \
             flow_data = json_object('consent_version', (SELECT notice_version FROM consent \
             WHERE student_id = ? ORDER BY accepted_at DESC LIMIT 1)), updated_at = ? \
             WHERE id = ? AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
        )
        .bind(link.student.clone())
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

/// Links the Channel Account `external_id` on Channel `channel` (a
/// [`crate::ChannelKind`] the Deployment has) to the Student `student_id`
/// (`student.id`) without Verification: an active Link with method
/// `manual` and `verified_by_staff_id` (the Staff id; `NULL` for
/// [`Actor::System`]), audit `link.create`. The account's open
/// challenges are cancelled.
///
/// When the Student has not accepted the current notice version, the
/// account is parked on the Consent Card (`awaiting_reconsent`): the notice
/// `kernel.link.manual_notice` and the Card are Pushed, the Menu stays
/// locked, and it unlocks only after accept (an Action sent before is
/// parked and replayed). When the Student already accepted it (on another
/// Channel), the notice is Pushed and the Menu unlocks at once.
///
/// `Err(Error::Conflict)` when the Student already has a live Link on this
/// Channel or the account already has one; `Err(Error::Invalid)` for an
/// unknown Channel or Student, or a Student who has left.
pub async fn manual_link(
    d: &Deployment,
    student_id: &str,
    channel: &str,
    external_id: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<LinkChange> {
    let s = d.services();
    let kind = d.channel_or_err(channel)?.kind();
    if external_id.trim().is_empty() {
        return Err(Error::Invalid("empty channel account id".into()));
    }
    let texts = Texts::load(s, d.catalog_arc(), d.config().default_locale.clone()).await?;
    let version = texts.consent_version(&d.config().consent.version);
    let student = read_student(s, student_id, &version).await?;
    let now = s.now_ms();
    let link_id = s.new_id()?;
    let state = if student.consented {
        "linked"
    } else {
        "awaiting_reconsent"
    };
    let account = "(SELECT id FROM channel_account WHERE channel = ? AND external_id = ?)";
    let after = serde_json::json!({
        "channel": channel,
        "external_id": external_id,
        "student_id": student_id,
        "method": "manual",
    });
    let target = Target::new("link", link_id.clone());
    let stmts = vec![
        Statement::new(
            "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
             VALUES (?, ?, ?, ?, ?) ON CONFLICT (channel, external_id) DO NOTHING",
        )
        .bind(s.new_id()?)
        .bind(channel)
        .bind(external_id)
        .bind(now)
        .bind(now),
        Statement::new(format!(
            "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
             verified_by_staff_id, linked_at, updated_at) \
             VALUES (?, ?, {account}, ?, 'active', 'manual', ?, ?, ?)"
        ))
        .bind(link_id.clone())
        .bind(student_id)
        .bind(channel)
        .bind(external_id)
        .bind(channel)
        .bind(staff_id(actor))
        .bind(now)
        .bind(now),
        Statement::new(format!(
            "UPDATE verification_challenge SET state = 'cancelled' \
             WHERE channel_account_id = {account} AND state = 'pending'"
        ))
        .bind(channel)
        .bind(external_id),
        Statement::new(
            "UPDATE channel_account SET flow_state = ?, flow_data = '{}', updated_at = ? \
             WHERE channel = ? AND external_id = ?",
        )
        .bind(state)
        .bind(now)
        .bind(channel)
        .bind(external_id),
        audit::entry(
            s,
            actor,
            "link.create",
            Some(&target),
            None,
            Some(after),
            ctx,
        )?,
    ];
    match s.db.batch(stmts).await {
        Ok(_) => {}
        Err(Error::Db(e)) if e.contains("UNIQUE") => {
            return Err(Error::Conflict(
                "a live Link already exists for this Student on this Channel or for this account"
                    .into(),
            ));
        }
        Err(e) => return Err(e),
    }
    let to = ChannelAccountId::new(kind, external_id);
    let texts = texts.in_locale(&student.locale);
    let mut msgs = vec![Message::text(
        texts.tf("kernel.link.manual_notice", &[("name", &student.name)]),
    )];
    let menu = if student.consented {
        Some(d.menu_with(|k| texts.t(k)))
    } else {
        msgs.push(flow::consent_card(
            d,
            &texts,
            channel,
            "kernel.consent.title",
        )?);
        None
    };
    let notified = notify(d, &to, menu, msgs, &student.locale).await;
    Ok(LinkChange { link_id, notified })
}

/// The revoke itself, for a caller's batch: the audit row `link.revoke`
/// (inserted only while the Link is live) and the Link update (run only
/// if that row exists). Returns the statements and the audit row's id, to
/// gate further statements on. The Link update is the second statement.
pub(crate) fn revoke_statements(
    s: &Services,
    link_id: &str,
    student: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<(Vec<Statement>, String)> {
    let now = s.now_ms();
    let by = match actor {
        Actor::Student(_) => "student",
        Actor::Staff(_) => "staff",
        _ => "system",
    };
    let after = serde_json::json!({ "status": "revoked", "student_id": student, "by": by });
    let target = Target::new("link", link_id);
    let entry = audit::entry(
        s,
        actor,
        "link.revoke",
        Some(&target),
        None,
        Some(after),
        ctx,
    )?;
    let gate = match entry.params.first() {
        Some(Value::Text(id)) => id.clone(),
        _ => return Err(Error::Invalid("audit entry without an id".into())),
    };
    let audit = when(
        entry,
        "EXISTS (SELECT 1 FROM link WHERE id = ? AND status <> 'revoked')",
    )?
    .bind(link_id);
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

/// `INSERT INTO t (..) VALUES (..)` as `INSERT INTO t (..) SELECT .. WHERE
/// cond`; bind the condition's params after.
fn when(stmt: Statement, cond: &str) -> Result<Statement> {
    let sql = stmt.sql.trim_end();
    let (head, values) = sql
        .rsplit_once(" VALUES (")
        .ok_or_else(|| Error::Invalid("not an INSERT .. VALUES statement".into()))?;
    let values = values
        .strip_suffix(')')
        .ok_or_else(|| Error::Invalid("not an INSERT .. VALUES statement".into()))?;
    let sql = format!("{head} SELECT {values} WHERE {cond}");
    Ok(Statement {
        sql: sql.into(),
        params: stmt.params,
    })
}

fn staff_id(actor: &Actor) -> Option<String> {
    match actor {
        Actor::Staff(id) => Some(id.clone()),
        _ => None,
    }
}

fn already_revoked() -> Error {
    Error::Conflict("the Link is already revoked".into())
}

/// Locks (`None`) or unlocks the Menu, then Pushes `msgs`. `true` when
/// both went through.
async fn notify(
    d: &Deployment,
    to: &ChannelAccountId,
    menu: Option<Menu>,
    msgs: Vec<Message>,
    locale: &Locale,
) -> bool {
    let Some(channel) = d.channel(to.channel.as_str()) else {
        return false;
    };
    let menu_set = channel.set_menu(to, menu.as_ref()).await.is_ok();
    let sent = d
        .deliver(to, None, &msgs, locale, Some(super::NAME))
        .await
        .is_ok();
    menu_set && sent
}

/// A Link with its account and the Student's Locale.
struct LinkInfo {
    id: String,
    status: String,
    student: String,
    account: String,
    channel: String,
    external_id: String,
    locale: Locale,
}

async fn read_link(s: &Services, link_id: &str) -> Result<LinkInfo> {
    let rows =
        s.db.query(
            Statement::new(
                "SELECT l.id, l.status, l.student_id, l.channel_account_id, ca.channel, \
                 ca.external_id, st.locale FROM link l \
                 JOIN channel_account ca ON ca.id = l.channel_account_id \
                 JOIN student st ON st.id = l.student_id WHERE l.id = ?",
            )
            .bind(link_id),
        )
        .await?;
    let text = |c: &str| rows.get(0, c).and_then(Value::as_text).map(str::to_string);
    let Some(id) = text("id") else {
        return Err(Error::Invalid(format!("no link {link_id}")));
    };
    Ok(LinkInfo {
        id,
        status: text("status").unwrap_or_default(),
        student: text("student_id").unwrap_or_default(),
        account: text("channel_account_id").unwrap_or_default(),
        channel: text("channel").unwrap_or_default(),
        external_id: text("external_id").unwrap_or_default(),
        locale: Locale(text("locale").unwrap_or_else(|| "th".into())),
    })
}

/// A Student a Manual Link may point at.
struct StudentInfo {
    name: String,
    locale: Locale,
    /// Has Consent for the current notice version.
    consented: bool,
}

async fn read_student(s: &Services, id: &str, version: &str) -> Result<StudentInfo> {
    let rows =
        s.db.query(
            Statement::new(
                "SELECT display_name, locale, status, EXISTS (SELECT 1 FROM consent c \
                 WHERE c.student_id = student.id AND c.notice_version = ?) AS cur \
                 FROM student WHERE id = ?",
            )
            .bind(version)
            .bind(id),
        )
        .await?;
    let text = |c: &str| rows.get(0, c).and_then(Value::as_text).map(str::to_string);
    match text("status").as_deref() {
        Some("active") => Ok(StudentInfo {
            name: text("display_name").unwrap_or_default(),
            locale: Locale(text("locale").unwrap_or_else(|| "th".into())),
            consented: rows.get(0, "cur").and_then(Value::as_i64) == Some(1),
        }),
        Some(_) => Err(Error::Invalid(format!("student {id} has left"))),
        None => Err(Error::Invalid(format!("no student {id}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::when;
    use crate::port_types::Statement;

    #[test]
    fn values_become_a_guarded_select() {
        let s = Statement::new("INSERT INTO t (a, b) VALUES (?, ?)")
            .bind(1_i64)
            .bind(2_i64);
        let g = when(s, "EXISTS (SELECT 1 FROM u WHERE id = ?)").unwrap();
        assert_eq!(
            g.sql,
            "INSERT INTO t (a, b) SELECT ?, ? WHERE EXISTS (SELECT 1 FROM u WHERE id = ?)"
        );
        assert_eq!(g.params.len(), 2);
        assert!(when(Statement::new("UPDATE t SET a = 1"), "1").is_err());
    }
}
