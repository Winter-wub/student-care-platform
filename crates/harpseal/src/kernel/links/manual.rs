//! Manual Link: Staff link a Channel Account to a Student without
//! Verification, after checking the Student's identity themselves.

use super::{LinkChange, R, conflict, notify, staff_id, text};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::delivery::ChannelAccountId;
use crate::deployment::Deployment;
use crate::kernel::flow;
use crate::kernel::text::Texts;
use crate::message::{Locale, Message};
use crate::port_types::{Statement, Value};
use crate::{Error, Services};

/// Links the Channel Account `external_id` (trimmed) on Channel `channel`
/// (a [`crate::ChannelKind`] the Deployment has) to the Student
/// `student_id` (`student.id`) without Verification: a Link with method
/// `manual` and `verified_by_staff_id` (the Staff id; `NULL` for any
/// other actor), audit `link.create`. The account must have written to
/// the bot before (its `channel_account` row exists), so a mistyped id
/// never creates an account. The account's open challenges are cancelled.
/// An account that blocked the bot gets an `unreachable` Link, which
/// becomes active on its next Inbound.
///
/// When the Student has not accepted the current notice version, the
/// account is parked on the Consent Card (`awaiting_reconsent`): the notice
/// `kernel.link.manual_notice` and the Card are Pushed, the Menu stays
/// locked, and it unlocks only after accept (an Action sent before is
/// parked and replayed). When the Student already accepted it (on another
/// Channel), only the notice is Pushed and the Menu unlocks at once.
///
/// `Err(Error::Conflict)` when the Student already has a live Link on this
/// Channel or the account already has one; `Err(Error::Invalid)` for an
/// unknown Channel, account or Student, or a Student who has left.
pub async fn manual_link(
    d: &Deployment,
    student_id: &str,
    channel: &str,
    external_id: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> R<LinkChange> {
    let s = d.services();
    let kind = d.channel_or_err(channel)?.kind();
    let external_id = external_id.trim();
    let account = read_account(s, channel, external_id).await?;
    let default = d.config().default_locale.clone();
    let texts = Texts::load(s, d.catalog_arc(), default.clone()).await?;
    let version = texts.consent_version(&d.config().consent.version);
    let student = read_student(s, student_id, &version, &default).await?;
    let now = s.now_ms();
    let link_id = s.new_id()?;
    let state = if student.consented {
        "linked"
    } else {
        "awaiting_reconsent"
    };
    let after = serde_json::json!({
        "channel": channel,
        "channel_account_id": account,
        "student_id": student_id,
        "method": "manual",
    });
    let target = Target::new("link", link_id.clone());
    let stmts = vec![
        Statement::new(
            "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
             verified_by_staff_id, linked_at, updated_at) VALUES (?, ?, ?, ?, \
             (SELECT CASE WHEN blocked_at IS NULL THEN 'active' ELSE 'unreachable' END \
             FROM channel_account WHERE id = ?), 'manual', ?, ?, ?)",
        )
        .bind(link_id.clone())
        .bind(student_id)
        .bind(account.clone())
        .bind(channel)
        .bind(account.clone())
        .bind(staff_id(actor))
        .bind(now)
        .bind(now),
        Statement::new(
            "UPDATE verification_challenge SET state = 'cancelled' \
             WHERE channel_account_id = ? AND state = 'pending'",
        )
        .bind(account.clone()),
        Statement::new(
            "UPDATE channel_account SET flow_state = ?, flow_data = '{}', updated_at = ? \
             WHERE id = ?",
        )
        .bind(state)
        .bind(now)
        .bind(account),
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
            return Err(conflict(
                "a live Link already exists for this Student on this Channel or for this account",
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
        let locale = d.catalog().effective(&student.locale);
        let texts = texts.in_locale(&locale);
        Some(d.menu_with(&locale, |k| texts.t(k)))
    } else {
        // Same titles as the chat flow's re-consent Card.
        let title = if student.any_consent {
            "kernel.consent.updated"
        } else {
            "kernel.consent.title"
        };
        msgs.push(flow::consent_card(d, &texts, channel, title)?);
        None
    };
    let notified = notify(d, &to, menu, msgs, &student.locale).await;
    Ok(LinkChange { link_id, notified })
}

/// `channel_account.id`; `Err(Error::Invalid)` when the account never
/// wrote to the bot (or the id is empty).
async fn read_account(s: &Services, channel: &str, external_id: &str) -> R<String> {
    if external_id.is_empty() {
        return Err(Error::Invalid("empty channel account id".into()));
    }
    let stmt =
        Statement::new("SELECT id FROM channel_account WHERE channel = ? AND external_id = ?")
            .bind(channel)
            .bind(external_id);
    text(&s.db.query(stmt).await?, "id")
        .ok_or_else(|| Error::Invalid(format!("no {channel} account {external_id}")))
}

/// A Student a Manual Link may point at.
struct StudentInfo {
    name: String,
    locale: Locale,
    /// Has Consent for the current notice version.
    consented: bool,
    /// Has Consent for any version.
    any_consent: bool,
}

async fn read_student(s: &Services, id: &str, version: &str, default: &Locale) -> R<StudentInfo> {
    let stmt = Statement::new(
        "SELECT display_name, locale, status, \
         EXISTS (SELECT 1 FROM consent c WHERE c.student_id = student.id \
         AND c.notice_version = ?) AS cur, \
         EXISTS (SELECT 1 FROM consent c WHERE c.student_id = student.id) AS any_c \
         FROM student WHERE id = ?",
    )
    .bind(version)
    .bind(id);
    let rows = s.db.query(stmt).await?;
    let flag = |c: &str| rows.get(0, c).and_then(Value::as_i64) == Some(1);
    match text(&rows, "status").as_deref() {
        Some("active") => Ok(StudentInfo {
            name: text(&rows, "display_name").unwrap_or_default(),
            locale: text(&rows, "locale").map_or_else(|| default.clone(), Locale),
            consented: flag("cur"),
            any_consent: flag("any_c"),
        }),
        Some(_) => Err(Error::Invalid(format!("student {id} has left"))),
        None => Err(Error::Invalid(format!("no student {id}"))),
    }
}
