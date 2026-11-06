//! After the send: `announcements.edit` replaces the Card already sent
//! where the Channel can edit (Telegram), `announcements.retract` deletes
//! it where the Channel can delete, within the platform's window.

use serde_json::{Value as Json, json};

use super::store;
use crate::delivery::{ChannelAccountId, PlatformMessageId};
use crate::manifest::Job;
use crate::message::{Locale, Message};
use crate::module::JobContext;
use crate::port_types::{Rows, Statement};
use crate::{Error, Result};

/// How long after the send a message can still be deleted (Telegram:
/// 48 hours); older ones are marked `too_old`.
pub(crate) const DELETE_WINDOW_MS: i64 = 48 * 3_600_000;

fn id_of(job: &Job) -> Result<String> {
    let p: Json = serde_json::from_str(&job.payload)
        .map_err(|_| Error::Invalid("job payload is not JSON".into()))?;
    p.get("id")
        .and_then(Json::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Invalid("job payload has no `id`".into()))
}

/// Sent deliveries with a platform message id: (row id, channel, external
/// id, Student Locale, sent_at, platform message id).
async fn sent_rows(cx: &JobContext, id: &str, extra: &'static str) -> Result<Rows> {
    let sql = format!(
        "SELECT d.id, d.channel, ca.external_id, s.locale, d.sent_at, d.platform_message_id \
         FROM announcement_delivery d JOIN channel_account ca ON ca.id = d.channel_account_id \
         JOIN student s ON s.id = d.student_id WHERE d.announcement_id = ? \
         AND d.status = 'sent' AND d.platform_message_id IS NOT NULL{extra} ORDER BY d.id"
    );
    cx.services.db.query(Statement::new(sql).bind(id)).await
}

fn account(cx: &JobContext, rows: &Rows, i: usize) -> Option<ChannelAccountId> {
    let channel = cx.deployment.channel(&store::text(rows, i, "channel"))?;
    Some(ChannelAccountId::new(
        channel.kind(),
        store::text(rows, i, "external_id"),
    ))
}

/// `announcements.edit`: every sent Card on a Channel with `editor()` is
/// replaced by the current texts in its Student's Locale; the counts go to
/// `announcement.edit_report`. Channels without it (LINE) are left alone.
pub(crate) async fn edit(cx: JobContext, job: Job) -> Result<()> {
    let id = id_of(&job)?;
    let s = &cx.services;
    let Some(row) = store::load(s, &id).await? else {
        return Ok(());
    };
    if row.status == "retracted" {
        return Ok(());
    }
    let texts = store::texts(s, &id).await?;
    let rows = sent_rows(&cx, &id, "").await?;
    let (mut edited, mut failed) = (0u32, 0u32);
    for i in 0..rows.len() {
        let Some(to) = account(&cx, &rows, i) else {
            continue;
        };
        let Some(editor) = cx
            .deployment
            .channel(to.channel.as_str())
            .and_then(|c| c.editor())
        else {
            continue;
        };
        let locale = texts.locale_for(&Locale::new(store::text(&rows, i, "locale")));
        let card = super::chat::card(
            cx.deployment.catalog(),
            &locale,
            &id,
            &texts,
            row.require_ack,
        )?;
        let pid = PlatformMessageId(store::text(&rows, i, "platform_message_id"));
        match editor.edit(&to, &pid, &Message::Card(card), &locale).await {
            Ok(()) => edited += 1,
            Err(_) => failed += 1,
        }
    }
    let report = json!({ "at": s.now_ms(), "edited": edited, "failed": failed });
    s.db.execute(
        Statement::new("UPDATE announcement SET edit_report = ? WHERE id = ?")
            .bind(report.to_string())
            .bind(id.as_str()),
    )
    .await?;
    Ok(())
}

/// `announcements.retract`: deletes every sent Card on a Channel with
/// `deleter()`; one sent more than 48 hours ago is marked `too_old`. Each
/// row's `removal` is set once, so a retry only does what is left.
pub(crate) async fn retract(cx: JobContext, job: Job) -> Result<()> {
    let id = id_of(&job)?;
    let s = &cx.services;
    let rows = sent_rows(&cx, &id, " AND d.removal IS NULL").await?;
    let now = s.now_ms();
    let mut again = None;
    for i in 0..rows.len() {
        let Some(to) = account(&cx, &rows, i) else {
            continue;
        };
        let Some(deleter) = cx
            .deployment
            .channel(to.channel.as_str())
            .and_then(|c| c.deleter())
        else {
            continue;
        };
        let sent_at = store::int(&rows, i, "sent_at").unwrap_or(0);
        let removal = if now - sent_at > DELETE_WINDOW_MS {
            "too_old"
        } else {
            let pid = PlatformMessageId(store::text(&rows, i, "platform_message_id"));
            match deleter.delete(&to, &pid).await {
                Ok(()) => "deleted",
                Err(e) if matches!(e.root(), Error::RateLimited { .. }) => {
                    again = Some(e);
                    break;
                }
                Err(_) => "failed",
            }
        };
        s.db.execute(
            Statement::new(
                "UPDATE announcement_delivery SET removal = ? WHERE id = ? AND removal IS NULL",
            )
            .bind(removal)
            .bind(store::text(&rows, i, "id")),
        )
        .await?;
    }
    again.map_or(Ok(()), Err)
}
