//! Staff writes. Each is one `Db::batch`: an audit row written only if
//! the announcement still has the status and version the page showed (a
//! compare-and-set, [`crate::audit::entry_if`]), then the changes gated on
//! that row. A lost race is `Error::Conflict`; nothing changes then.

use serde_json::json;

use super::deliver::{deliver_key, parent_job};
pub(crate) use super::draft::Draft;
use super::draft::{changed_locales, text_rows};
use super::gate::{GATED, JobReq, bump, cas, load_err};
use super::notice::notice_key;
use super::store;
use super::{EDIT, NAME, NOTICE, RETRACT};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::port_types::Statement;
use crate::{Deployment, Error, Result};

/// Creates a draft; returns its id. Audit `announcement.create`.
pub(crate) async fn create(
    d: &Deployment,
    draft: &Draft,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<String> {
    let s = d.services();
    draft.audience.check(&d.config().roster)?;
    let id = s.new_id()?;
    let now = s.now_ms();
    let created_by = match actor {
        Actor::Staff(id) => Some(id.clone()),
        _ => None,
    };
    let mut stmts = vec![
        audit::entry(
            s,
            actor,
            "announcement.create",
            Some(&Target::new(NAME, &id)),
            None,
            Some(draft.summary()),
            ctx,
        )?,
        Statement::new(
            "INSERT INTO announcement (id, status, audience, send_at, expires_at, require_ack, \
             created_by, created_at, updated_at) VALUES (?, 'draft', ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(serde_json::to_string(&draft.audience).unwrap_or_else(|_| "{}".into()))
        .bind(draft.send_at)
        .bind(draft.expires_at)
        .bind(draft.require_ack)
        .bind(created_by)
        .bind(now)
        .bind(now),
    ];
    stmts.extend(text_rows(&id, draft, None));
    s.db.batch(stmts).await?;
    Ok(id)
}

/// Edits announcement `id` seen at `version`. A draft or a schedule
/// takes everything (a schedule moves its Job); a sent announcement takes
/// the texts and the expiry, and Job `announcements.edit` replaces the
/// Cards where the Channel can edit. With [`Draft::notify`], a sent one
/// whose text changed and that does not expire with this save also starts
/// the edit notice ([`super::notice`]): `notice_report` reset to the new
/// version (stopping an older run) and Job `announcements.notice` for the
/// changed Locales. Audit `announcement.update`.
pub(crate) async fn update(
    d: &Deployment,
    id: &str,
    version: i64,
    draft: &Draft,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<()> {
    let s = d.services();
    let mut row = load_err(store::load(s, id).await?)?;
    row.version = version;
    let now = s.now_ms();
    let next = version + 1;
    let full = matches!(row.status.as_str(), "draft" | "scheduled");
    if full {
        draft.audience.check(&d.config().roster)?;
    }
    let mut jobs = Vec::new();
    if row.status == "scheduled" {
        let at = draft
            .send_at
            .filter(|t| *t > now)
            .ok_or_else(|| Error::Invalid("send_at".into()))?;
        jobs.push(parent_job(id, row.round, next, Some(at)));
    }
    // Only Cards whose visible text changed are edited (and notified).
    let locales = if row.status == "sent" {
        changed_locales(&store::texts(s, id).await?, draft)
    } else {
        Vec::new()
    };
    if !locales.is_empty() {
        jobs.push(JobReq {
            key: format!("{NAME}:{id}:edit:v{next}"),
            kind: EDIT,
            payload: json!({ "id": id, "version": next, "locales": locales }),
            run_at: None,
        });
    }
    // A notice only for a changed text of an announcement still open
    // after this save (the page refuses the box otherwise).
    let open_after = draft.expires_at.is_none_or(|e| e > now);
    let notify = draft.notify && !locales.is_empty() && open_after;
    if notify {
        jobs.push(JobReq {
            key: notice_key(id, next),
            kind: NOTICE,
            payload: json!({ "id": id, "version": next, "locales": locales }),
            run_at: None,
        });
    }
    let old_key = deliver_key(id, row.round, version);
    let audience = serde_json::to_string(&draft.audience).unwrap_or_else(|_| "{}".into());
    let changes = |gate: &str| {
        let mut v = Vec::new();
        if full {
            v.push(
                Statement::new(format!(
                    "UPDATE announcement SET audience = ?, send_at = ?, expires_at = ?, \
                     require_ack = ?, version = version + 1, updated_at = {now} \
                     WHERE id = ? AND {GATED}"
                ))
                .bind(audience)
                .bind(draft.send_at)
                .bind(draft.expires_at)
                .bind(draft.require_ack)
                .bind(id)
                .bind(gate),
            );
            v.push(
                Statement::new(format!(
                    "DELETE FROM job WHERE unique_key = ? AND state = 'queued' AND {GATED}"
                ))
                .bind(old_key)
                .bind(gate),
            );
        } else {
            v.push(
                Statement::new(format!(
                    "UPDATE announcement SET expires_at = ?, version = version + 1, \
                     updated_at = {now} WHERE id = ? AND {GATED}"
                ))
                .bind(draft.expires_at)
                .bind(id)
                .bind(gate),
            );
        }
        v.push(
            Statement::new(format!(
                "DELETE FROM announcement_text WHERE announcement_id = ? AND {GATED}"
            ))
            .bind(id)
            .bind(gate),
        );
        v.extend(text_rows(id, draft, Some(gate)));
        if notify {
            v.push(
                Statement::new(format!(
                    "UPDATE announcement SET notice_report = json_object('version', {next}, \
                     'at', {now}, 'sent', 0, 'failed', 0, 'total', NULL) WHERE id = ? AND {GATED}"
                ))
                .bind(id)
                .bind(gate),
            );
        }
        v
    };
    let from: &[&str] = &["draft", "scheduled", "sent"];
    cas(
        d,
        &row,
        from,
        "announcement.update",
        draft.summary(),
        actor,
        ctx,
        changes,
        jobs,
    )
    .await
}

/// Confirms a draft: scheduled for its `send_at` when that is still
/// ahead, otherwise sending at once. Audit `announcement.send` or
/// `announcement.schedule`. Returns whether it was scheduled.
pub(crate) async fn confirm(
    d: &Deployment,
    id: &str,
    version: i64,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<bool> {
    let s = d.services();
    let mut row = load_err(store::load(s, id).await?)?;
    row.version = version;
    let now = s.now_ms();
    let later = row.send_at.filter(|t| *t > now);
    let (status, action) = match later {
        Some(_) => ("scheduled", "announcement.schedule"),
        None => ("sending", "announcement.send"),
    };
    let job = parent_job(id, row.round, version + 1, later);
    let mut after = json!({ "status": status, "send_at": later });
    if !row.audience.students.is_empty() {
        // The size of the hand-picked list, never the ids.
        after["hand_picked"] = row.audience.students.len().into();
    }
    let set = format!("status = '{status}'");
    let changes = |gate: &str| vec![bump(&set, &row, now, gate)];
    cas(
        d,
        &row,
        &["draft"],
        action,
        after,
        actor,
        ctx,
        changes,
        vec![job],
    )
    .await?;
    Ok(later.is_some())
}

/// A schedule back to a draft; its queued Job is deleted. Audit
/// `announcement.cancel`.
pub(crate) async fn cancel(
    d: &Deployment,
    id: &str,
    version: i64,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<()> {
    let s = d.services();
    let mut row = load_err(store::load(s, id).await?)?;
    row.version = version;
    let now = s.now_ms();
    let key = deliver_key(id, row.round, version);
    let changes = |gate: &str| {
        vec![
            bump("status = 'draft'", &row, now, gate),
            Statement::new(format!(
                "DELETE FROM job WHERE unique_key = ? AND state = 'queued' AND {GATED}"
            ))
            .bind(key)
            .bind(gate),
        ]
    };
    let after = json!({ "status": "draft" });
    cas(
        d,
        &row,
        &["scheduled"],
        "announcement.cancel",
        after,
        actor,
        ctx,
        changes,
        vec![],
    )
    .await
}

/// "ส่งซ้ำ": a new round to the Audience as it is now, with a new Job key.
/// An expired announcement is not resent (`Error::Conflict`): Students
/// could not open it. Audit `announcement.resend`.
pub(crate) async fn resend(
    d: &Deployment,
    id: &str,
    version: i64,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<()> {
    let s = d.services();
    let mut row = load_err(store::load(s, id).await?)?;
    row.version = version;
    let now = s.now_ms();
    if !super::notice::open(&row, now) {
        return Err(Error::Conflict("announcement expired".into()));
    }
    let round = row.round + 1;
    let job = parent_job(id, round, version + 1, None);
    let set = format!("status = 'sending', round = {round}");
    let changes = |gate: &str| vec![bump(&set, &row, now, gate)];
    let after = json!({ "status": "sending", "round": round });
    cas(
        d,
        &row,
        &["sent"],
        "announcement.resend",
        after,
        actor,
        ctx,
        changes,
        vec![job],
    )
    .await
}

/// Retracts: hidden from the Menu and the Mini App, a late ack answers
/// "ข่าวนี้ถูกถอนแล้ว", and Job `announcements.retract` deletes the Cards
/// where the Channel can. No notice is sent. Audit `announcement.retract`.
pub(crate) async fn retract(
    d: &Deployment,
    id: &str,
    version: i64,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<()> {
    let s = d.services();
    let mut row = load_err(store::load(s, id).await?)?;
    row.version = version;
    let now = s.now_ms();
    // Keyed per version: a lost race inserts nothing and leaves the key free.
    let next = version + 1;
    let job = JobReq {
        key: format!("{NAME}:{id}:retract:v{next}"),
        kind: RETRACT,
        payload: json!({ "id": id, "version": next }),
        run_at: None,
    };
    let set = format!("status = 'retracted', retracted_at = {now}");
    let changes = |gate: &str| vec![bump(&set, &row, now, gate)];
    let after = json!({ "status": "retracted" });
    let from: &[&str] = &["scheduled", "sending", "sent"];
    cas(
        d,
        &row,
        from,
        "announcement.retract",
        after,
        actor,
        ctx,
        changes,
        vec![job],
    )
    .await
}
