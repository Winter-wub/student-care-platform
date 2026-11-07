//! After the send: `announcements.edit` replaces the Card already sent
//! where the Channel can edit (Telegram), `announcements.retract` deletes
//! it where the Channel can delete, within the platform's window.
//!
//! Both work like the deliver children (#49): the Job a write enqueued
//! fans out one chunk Job (`announcements.edit.chunk`,
//! `announcements.retract.chunk`, in [`super::maint_chunk`]) per [`CHUNK`]
//! delivery rows per Channel that can do it, only for chunks with a row
//! left to do, [`PER_MINUTE`] chunks a minute, each with a unique key. A
//! chunk stopped by `Error::RateLimited` enqueues its continuation from
//! the last row it handled, due after `retry_after_secs`: the progress
//! lives in the Job payload's cursor, not on the delivery row. The retract
//! also runs a late pass [`LATE_PASS_MS`] later for Cards a deliver child
//! sent while the retract ran.
//!
//! A deliver child that dies between the platform accepting the send and
//! its outcome batch leaves the row `pending` without a
//! `platform_message_id`: no pass can delete that Card (the late pass
//! marks such rows `skipped`).

use serde_json::{Value as Json, json};

use super::deliver::PACED_CHUNK;
use super::store::{self, Row};
use super::{EDIT_CHUNK, NAME, RETRACT, RETRACT_CHUNK};
use crate::delivery::Timestamp;
use crate::manifest::Job;
use crate::message::Locale;
use crate::module::JobContext;
use crate::port_types::Statement;
use crate::{Error, Result};

/// How long after the send a message can still be deleted (Telegram:
/// 48 hours); older ones are marked `too_old`.
pub(crate) const DELETE_WINDOW_MS: i64 = 48 * 3_600_000;
/// Delivery rows per chunk Job.
pub(crate) const CHUNK: usize = PACED_CHUNK;
/// Chunks started per minute per Channel (25 × 10 = 250 calls a minute).
pub(crate) const PER_MINUTE: usize = 10;
/// When the retract's late pass runs: past the Job lock (15 minutes) of a
/// deliver child that read `sending` before the retract, so that child has
/// finished or lost its lock (and stops: the status is re-read) by then.
pub(crate) const LATE_PASS_MS: i64 = 16 * 60_000;

/// Which maintenance a Job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Op {
    Edit,
    Retract,
}

impl Op {
    pub(super) fn chunk_kind(self) -> &'static str {
        match self {
            Op::Edit => EDIT_CHUNK,
            Op::Retract => RETRACT_CHUNK,
        }
    }
    pub(super) fn can(self, cx: &JobContext, channel: &str) -> bool {
        cx.deployment.channel(channel).is_some_and(|c| match self {
            Op::Edit => c.editor().is_some(),
            Op::Retract => c.deleter().is_some(),
        })
    }
    /// The rows a chunk works on, beyond `status = 'sent'` with a
    /// platform message id (`x` is the delivery alias).
    pub(super) fn todo(self) -> &'static str {
        match self {
            Op::Edit => "",
            Op::Retract => " AND x.removal IS NULL",
        }
    }
}

pub(super) fn get_str(p: &Json, k: &str) -> String {
    p.get(k)
        .and_then(Json::as_str)
        .unwrap_or_default()
        .to_string()
}

pub(super) fn get_int(p: &Json, k: &str) -> i64 {
    p.get(k).and_then(Json::as_i64).unwrap_or(0)
}

/// The payload's announcement id and version.
pub(super) fn parse(job: &Job) -> Result<(Json, String, i64)> {
    let p: Json = serde_json::from_str(&job.payload)
        .map_err(|_| Error::Invalid("job payload is not JSON".into()))?;
    let id = p
        .get("id")
        .and_then(Json::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Invalid("job payload has no `id`".into()))?;
    let version = p.get("version").and_then(Json::as_i64).unwrap_or(-1);
    Ok((p, id, version))
}

/// The announcement while `op` still applies. A retract: still
/// `retracted` at the retract's version. An edit: `sent` (or `sending`
/// again, a resend) at the edit's version or a later one; a later save
/// bumps the version without undoing the edit, and the chunks load the
/// current texts. A retract stops it.
pub(super) async fn current(
    cx: &JobContext,
    op: Op,
    id: &str,
    version: i64,
) -> Result<Option<Row>> {
    Ok(store::load(&cx.services, id).await?.filter(|r| match op {
        Op::Edit => matches!(r.status.as_str(), "sent" | "sending") && r.version >= version,
        Op::Retract => r.status == "retracted" && r.version == version,
    }))
}

/// One chunk Job per [`CHUNK`] delivery rows (every round, every status,
/// in id order: a set that does not move while the Op applies) per
/// Channel that can do `op`, for the chunks holding a row to do; paced
/// [`PER_MINUTE`] a minute per Channel. `extra` is merged into each
/// payload. Keys are `<base>:<channel>:<n>`, so a re-run inserts nothing.
async fn fan_out(
    cx: &JobContext,
    op: Op,
    id: &str,
    version: i64,
    base: &str,
    extra: &Json,
) -> Result<Vec<Statement>> {
    let s = &cx.services;
    let sql = format!(
        "SELECT channel, n FROM (SELECT x.*, (row_number() OVER \
         (PARTITION BY x.channel ORDER BY x.id) - 1) / ? AS n FROM announcement_delivery x \
         WHERE x.announcement_id = ?) x WHERE x.status = 'sent' \
         AND x.platform_message_id IS NOT NULL{} GROUP BY channel, n ORDER BY channel, n",
        op.todo()
    );
    let size = i64::try_from(CHUNK).unwrap_or(25);
    let rows = s.db.query(Statement::new(sql).bind(size).bind(id)).await?;
    let now = s.now_ms();
    let mut stmts = Vec::new();
    let mut paced = 0usize;
    let mut last_channel = String::new();
    for i in 0..rows.len() {
        let channel = store::text(&rows, i, "channel");
        if !op.can(cx, &channel) {
            continue;
        }
        if channel != last_channel {
            paced = 0;
            last_channel.clone_from(&channel);
        }
        let n = store::int(&rows, i, "n").unwrap_or(0);
        let key = format!("{base}:{channel}:{n}");
        let minute = i64::try_from(paced / PER_MINUTE).unwrap_or(0);
        paced += 1;
        let mut payload = json!({
            "id": id, "version": version, "channel": channel,
            "offset": n * size, "size": size, "after": "", "key": key, "hop": 0,
        });
        if let (Some(p), Some(e)) = (payload.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        let at = Timestamp(now + minute * 60_000);
        let (_, stmt) = cx
            .jobs
            .insert_unique(&key, op.chunk_kind(), &payload, Some(at))?;
        stmts.push(stmt);
    }
    Ok(stmts)
}

/// Whether a delivery in Student Locale `own` gets edited: its text (or
/// the Thai it falls back to) is among the changed `locales`.
pub(super) fn wants_edit(texts: &store::Texts, changed: &[&str], own: &str) -> bool {
    let locale = texts.locale_for(&Locale::new(own));
    changed.contains(&own) || changed.contains(&locale.as_str())
}

/// The payload's changed Locales.
pub(super) fn changed(p: &Json) -> Vec<&str> {
    p.get("locales")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(Json::as_str).collect())
        .unwrap_or_default()
}

/// Sent Cards the edit will try: on a Channel with `editor()`, in a
/// changed Locale.
async fn edit_total(cx: &JobContext, id: &str, p: &Json) -> Result<i64> {
    let s = &cx.services;
    let rows =
        s.db.query(
            Statement::new(
                "SELECT d.channel, s.locale, count(*) AS n FROM announcement_delivery d \
                 JOIN student s ON s.id = d.student_id WHERE d.announcement_id = ? \
                 AND d.status = 'sent' AND d.platform_message_id IS NOT NULL \
                 GROUP BY d.channel, s.locale",
            )
            .bind(id),
        )
        .await?;
    let texts = store::texts(s, id).await?;
    let changed = changed(p);
    Ok((0..rows.len())
        .filter(|&i| {
            Op::Edit.can(cx, &store::text(&rows, i, "channel"))
                && wants_edit(&texts, &changed, &store::text(&rows, i, "locale"))
        })
        .map(|i| store::int(&rows, i, "n").unwrap_or(0))
        .sum())
}

/// `announcements.edit`: while the edit applies ([`current`]), starts
/// `announcement.edit_report` for this version (`edited` and `failed` at
/// zero, `total` the Cards it will try) unless a re-run or a later edit
/// already did, and fans out the chunks. Channels without `editor()`
/// (LINE) are left alone.
pub(crate) async fn edit(cx: JobContext, job: Job) -> Result<()> {
    let (p, id, version) = parse(&job)?;
    let s = &cx.services;
    if current(&cx, Op::Edit, &id, version).await?.is_none() {
        return Ok(());
    }
    let total = edit_total(&cx, &id, &p).await?;
    let mut stmts = vec![
        Statement::new(
            "UPDATE announcement SET edit_report = json_object('at', ?, 'version', ?, \
             'edited', 0, 'failed', 0, 'total', ?) WHERE id = ? \
             AND coalesce(json_extract(edit_report, '$.version'), -1) < ?",
        )
        .bind(s.now_ms())
        .bind(version)
        .bind(total)
        .bind(id.as_str())
        .bind(version),
    ];
    let base = format!("{NAME}:{id}:edit:v{version}");
    let extra = json!({ "locales": p.get("locales").cloned().unwrap_or(Json::Null) });
    stmts.extend(fan_out(&cx, Op::Edit, &id, version, &base, &extra).await?);
    s.db.batch(stmts).await?;
    Ok(())
}

/// `announcements.retract`: while the announcement is `retracted` at the
/// retract's version, fans out the chunks, and (the first pass only)
/// enqueues the late pass, the same Job with `"late": true`, due
/// [`LATE_PASS_MS`] later: it deletes Cards a deliver child sent after
/// the first pass read the rows (rows already handled have `removal` set,
/// so their chunks are not enqueued again) and marks rows still `pending`
/// `skipped` (`error = 'retracted'`): they will never be sent.
pub(crate) async fn retract(cx: JobContext, job: Job) -> Result<()> {
    let (p, id, version) = parse(&job)?;
    let s = &cx.services;
    if current(&cx, Op::Retract, &id, version).await?.is_none() {
        return Ok(());
    }
    let late = p.get("late").and_then(Json::as_bool).unwrap_or(false);
    let first = format!("{NAME}:{id}:retract:v{version}");
    let base = if late { format!("{first}:late") } else { first };
    let mut stmts = fan_out(&cx, Op::Retract, &id, version, &base, &json!({})).await?;
    if late {
        stmts.push(
            Statement::new(
                "UPDATE announcement_delivery SET status = 'skipped', error = 'retracted' \
                 WHERE announcement_id = ? AND status = 'pending'",
            )
            .bind(id.as_str()),
        );
    } else {
        let at = Timestamp(s.now_ms() + LATE_PASS_MS);
        let payload = json!({ "id": id, "version": version, "late": true });
        let key = format!("{base}:late");
        stmts.push(cx.jobs.insert_unique(&key, RETRACT, &payload, Some(at))?.1);
    }
    s.db.batch(stmts).await?;
    Ok(())
}

/// A maintenance Job (edit or retract, any version, chunks and late pass
/// included) of announcement `id` is waiting or running: the detail page
/// shows "in progress".
pub(crate) async fn in_progress(s: &crate::Services, id: &str) -> Result<bool> {
    let rows =
        s.db.query(
            Statement::new(
                "SELECT 1 AS x FROM job WHERE state IN ('queued', 'running') \
                 AND (unique_key LIKE ? || ':' || ? || ':edit:v%' \
                 OR unique_key LIKE ? || ':' || ? || ':retract:v%') LIMIT 1",
            )
            .bind(NAME)
            .bind(id)
            .bind(NAME)
            .bind(id),
        )
        .await?;
    Ok(!rows.is_empty())
}
