//! After the send: `announcements.edit` replaces the Card already sent
//! where the Channel can edit (Telegram), `announcements.retract` deletes
//! it where the Channel can delete, within the platform's window.
//!
//! Both work like the deliver children (#49): the Job a write enqueued
//! fans out one chunk Job (`announcements.edit.chunk`,
//! `announcements.retract.chunk`) per [`CHUNK`] delivery rows per Channel
//! that can do it, [`PER_MINUTE`] chunks a minute, each with a unique key.
//! A chunk stopped by `Error::RateLimited` enqueues its continuation from
//! the last row it handled, due after `retry_after_secs`. The retract
//! also runs a late pass [`LATE_PASS_MS`] later for Cards a deliver child
//! sent while the retract ran.

use serde_json::{Value as Json, json};

use super::deliver::PACED_CHUNK;
use super::store::{self, Row};
use super::{EDIT_CHUNK, NAME, RETRACT, RETRACT_CHUNK};
use crate::delivery::{ChannelAccountId, PlatformMessageId, Timestamp};
use crate::manifest::Job;
use crate::message::{Locale, Message};
use crate::module::JobContext;
use crate::port_types::{Rows, Statement};
use crate::{Error, Result};

/// How long after the send a message can still be deleted (Telegram:
/// 48 hours); older ones are marked `too_old`.
pub(crate) const DELETE_WINDOW_MS: i64 = 48 * 3_600_000;
/// Delivery rows per chunk Job.
pub(crate) const CHUNK: usize = PACED_CHUNK;
/// Chunks started per minute (25 × 10 = 250 calls a minute).
pub(crate) const PER_MINUTE: usize = 10;
/// When the retract's late pass runs: past the Job lock (15 minutes) of a
/// deliver child that read `sending` before the retract, so that child has
/// finished or lost its lock (and stops: the status is re-read) by then.
pub(crate) const LATE_PASS_MS: i64 = 16 * 60_000;
/// Continuations one chunk may chain; past it a rate limit fails the run
/// and the Job retries with the runner's backoff.
const MAX_HOPS: i64 = 50;

/// Which maintenance a Job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Edit,
    Retract,
}

impl Op {
    /// The announcement's status while this Op is still current.
    fn status(self) -> &'static str {
        match self {
            Op::Edit => "sent",
            Op::Retract => "retracted",
        }
    }
    fn chunk_kind(self) -> &'static str {
        match self {
            Op::Edit => EDIT_CHUNK,
            Op::Retract => RETRACT_CHUNK,
        }
    }
    fn can(self, cx: &JobContext, channel: &str) -> bool {
        cx.deployment.channel(channel).is_some_and(|c| match self {
            Op::Edit => c.editor().is_some(),
            Op::Retract => c.deleter().is_some(),
        })
    }
}

fn get_str(p: &Json, k: &str) -> String {
    p.get(k)
        .and_then(Json::as_str)
        .unwrap_or_default()
        .to_string()
}

fn get_int(p: &Json, k: &str) -> i64 {
    p.get(k).and_then(Json::as_i64).unwrap_or(0)
}

/// The payload's announcement id and version.
fn parse(job: &Job) -> Result<(Json, String, i64)> {
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

/// The announcement, only while it still has `status` at `version`: the
/// write that enqueued the Job is still the latest one.
async fn current(cx: &JobContext, id: &str, status: &str, version: i64) -> Result<Option<Row>> {
    Ok(store::load(&cx.services, id)
        .await?
        .filter(|r| r.status == status && r.version == version))
}

/// One chunk Job per [`CHUNK`] delivery rows (every round, every status:
/// a set that does not move while the Op is current) per Channel that can
/// do `op`; `extra` is merged into each payload.
async fn fan_out(
    cx: &JobContext,
    op: Op,
    id: &str,
    version: i64,
    base: &str,
    extra: &Json,
) -> Result<Vec<Statement>> {
    let s = &cx.services;
    let rows =
        s.db.query(
            Statement::new(
                "SELECT channel, count(*) AS n FROM announcement_delivery \
                 WHERE announcement_id = ? GROUP BY channel ORDER BY channel",
            )
            .bind(id),
        )
        .await?;
    let now = s.now_ms();
    let mut stmts = Vec::new();
    for i in 0..rows.len() {
        let channel = store::text(&rows, i, "channel");
        if !op.can(cx, &channel) {
            continue;
        }
        let total = usize::try_from(store::int(&rows, i, "n").unwrap_or(0)).unwrap_or(0);
        for n in 0..total.div_ceil(CHUNK) {
            let key = format!("{base}:{channel}:{n}");
            let minute = i64::try_from(n / PER_MINUTE).unwrap_or(0);
            let mut payload = json!({
                "id": id, "version": version, "channel": channel,
                "offset": n * CHUNK, "size": CHUNK, "after": "", "key": key, "hop": 0,
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
    }
    Ok(stmts)
}

/// `announcements.edit`: while the announcement is still `sent` at the
/// edit's version (a later edit has its own Job), starts
/// `announcement.edit_report` at zero for this version and fans out the
/// chunks. Channels without `editor()` (LINE) are left alone.
pub(crate) async fn edit(cx: JobContext, job: Job) -> Result<()> {
    let (p, id, version) = parse(&job)?;
    let s = &cx.services;
    if current(&cx, &id, "sent", version).await?.is_none() {
        return Ok(());
    }
    // A re-run of this Job keeps the counts its chunks already added.
    let mut stmts = vec![
        Statement::new(
            "UPDATE announcement SET edit_report = json_object('at', ?, 'version', ?, \
             'edited', 0, 'failed', 0) WHERE id = ? AND (edit_report IS NULL \
             OR json_extract(edit_report, '$.version') IS NOT ?)",
        )
        .bind(s.now_ms())
        .bind(version)
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
/// the first pass read the rows. Rows already handled have `removal` set
/// and are not touched again.
pub(crate) async fn retract(cx: JobContext, job: Job) -> Result<()> {
    let (p, id, version) = parse(&job)?;
    let s = &cx.services;
    if current(&cx, &id, "retracted", version).await?.is_none() {
        return Ok(());
    }
    let late = p.get("late").and_then(Json::as_bool).unwrap_or(false);
    let first = format!("{NAME}:{id}:retract:v{version}");
    let base = if late { format!("{first}:late") } else { first };
    let mut stmts = fan_out(&cx, Op::Retract, &id, version, &base, &json!({})).await?;
    if !late {
        let at = Timestamp(s.now_ms() + LATE_PASS_MS);
        let payload = json!({ "id": id, "version": version, "late": true });
        let key = format!("{base}:late");
        stmts.push(cx.jobs.insert_unique(&key, RETRACT, &payload, Some(at))?.1);
    }
    if !stmts.is_empty() {
        s.db.batch(stmts).await?;
    }
    Ok(())
}

/// The chunk's rows still to do, after the cursor `after`.
async fn chunk_rows(cx: &JobContext, id: &str, p: &Json, op: Op) -> Result<Rows> {
    let extra = if op == Op::Retract {
        " AND x.removal IS NULL"
    } else {
        ""
    };
    let sql = format!(
        "SELECT x.id, ca.external_id, s.locale, x.sent_at, x.platform_message_id, x.acked_at \
         FROM (SELECT * FROM announcement_delivery WHERE announcement_id = ? AND channel = ? \
         ORDER BY id LIMIT ? OFFSET ?) x JOIN channel_account ca ON ca.id = x.channel_account_id \
         JOIN student s ON s.id = x.student_id WHERE x.status = 'sent' \
         AND x.platform_message_id IS NOT NULL AND x.id > ?{extra} ORDER BY x.id"
    );
    let st = Statement::new(sql)
        .bind(id)
        .bind(get_str(p, "channel"))
        .bind(get_int(p, "size"))
        .bind(get_int(p, "offset"))
        .bind(get_str(p, "after"));
    cx.services.db.query(st).await
}

/// `announcements.edit.chunk`: replaces each sent Card of the chunk whose
/// text changed (the Student's Locale, or the Thai it falls back to, is in
/// the payload's `locales`) with the current texts, and adds the counts
/// to `edit_report`.
pub(crate) async fn edit_chunk(cx: JobContext, job: Job) -> Result<()> {
    chunk(cx, job, Op::Edit).await
}

/// `announcements.retract.chunk`: deletes each sent Card of the chunk;
/// one sent more than 48 hours ago is marked `too_old`. Each row's
/// `removal` is set once, so a re-run only does what is left.
pub(crate) async fn retract_chunk(cx: JobContext, job: Job) -> Result<()> {
    chunk(cx, job, Op::Retract).await
}

async fn chunk(cx: JobContext, job: Job, op: Op) -> Result<()> {
    let (p, id, version) = parse(&job)?;
    let Some(row) = current(&cx, &id, op.status(), version).await? else {
        return Ok(());
    };
    let channel = get_str(&p, "channel");
    let (Some(ch), true) = (cx.deployment.channel(&channel), op.can(&cx, &channel)) else {
        return Ok(());
    };
    let kind = ch.kind();
    let s = &cx.services;
    let rows = chunk_rows(&cx, &id, &p, op).await?;
    let texts = store::texts(s, &id).await?;
    let changed: Vec<&str> = p
        .get("locales")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(Json::as_str).collect())
        .unwrap_or_default();
    let now = s.now_ms();
    let (mut edited, mut failed) = (0i64, 0i64);
    let mut done = get_str(&p, "after");
    let mut limited: Option<Error> = None;
    for i in 0..rows.len() {
        let to = ChannelAccountId::new(kind, store::text(&rows, i, "external_id"));
        let pid = PlatformMessageId(store::text(&rows, i, "platform_message_id"));
        let delivery = store::text(&rows, i, "id");
        let result = match op {
            Op::Edit => {
                let own = store::text(&rows, i, "locale");
                let locale = texts.locale_for(&Locale::new(own.as_str()));
                if !changed.contains(&own.as_str()) && !changed.contains(&locale.as_str()) {
                    done = delivery;
                    continue;
                }
                let acked = store::int(&rows, i, "acked_at").is_some();
                let ack = row.require_ack && !acked;
                let card = super::chat::card(cx.deployment.catalog(), &locale, &id, &texts, ack)?;
                let Some(editor) = ch.editor() else { break };
                let msg = Message::Card(card);
                editor
                    .edit(&to, &pid, &msg, &locale)
                    .await
                    .map(|()| "edited")
            }
            Op::Retract => {
                let sent_at = store::int(&rows, i, "sent_at").unwrap_or(0);
                let Some(deleter) = ch.deleter() else { break };
                if now - sent_at > DELETE_WINDOW_MS {
                    Ok("too_old")
                } else {
                    deleter.delete(&to, &pid).await.map(|()| "deleted")
                }
            }
        };
        let removal = match result {
            Err(e) if matches!(e.root(), Error::RateLimited { .. }) => {
                limited = Some(e);
                break;
            }
            Ok(r) => {
                edited += 1;
                r
            }
            Err(_) => {
                failed += 1;
                "failed"
            }
        };
        if op == Op::Retract {
            s.db.execute(
                Statement::new(
                    "UPDATE announcement_delivery SET removal = ? WHERE id = ? \
                     AND removal IS NULL",
                )
                .bind(removal)
                .bind(delivery.as_str()),
            )
            .await?;
        }
        done = delivery;
    }
    if op == Op::Edit && edited + failed > 0 {
        add_to_report(&cx, &id, version, edited, failed).await?;
    }
    match limited {
        Some(e) => continue_after(&cx, op, &p, &done, e).await,
        None => Ok(()),
    }
}

/// Adds to `edit_report` while it is still this version's.
async fn add_to_report(
    cx: &JobContext,
    id: &str,
    version: i64,
    edited: i64,
    failed: i64,
) -> Result<()> {
    let s = &cx.services;
    s.db.execute(
        Statement::new(
            "UPDATE announcement SET edit_report = json_set(edit_report, '$.at', ?, \
             '$.edited', json_extract(edit_report, '$.edited') + ?, \
             '$.failed', json_extract(edit_report, '$.failed') + ?) \
             WHERE id = ? AND json_extract(edit_report, '$.version') = ?",
        )
        .bind(s.now_ms())
        .bind(edited)
        .bind(failed)
        .bind(id)
        .bind(version),
    )
    .await?;
    Ok(())
}

/// A rate limit stopped the chunk: its continuation resumes after `done`
/// (the last row handled), due after `retry_after_secs`, keyed
/// `<chunk key>:<hop>` so a re-run of this Job enqueues it once.
async fn continue_after(cx: &JobContext, op: Op, p: &Json, done: &str, e: Error) -> Result<()> {
    let hop = get_int(p, "hop") + 1;
    if hop > MAX_HOPS {
        return Err(e);
    }
    let secs = match e.root() {
        Error::RateLimited { retry_after_secs } => i64::from(*retry_after_secs).max(1),
        _ => 1,
    };
    let mut next = p.clone();
    if let Some(o) = next.as_object_mut() {
        o.insert("after".into(), json!(done));
        o.insert("hop".into(), json!(hop));
    }
    let key = format!("{}:{hop}", get_str(p, "key"));
    let at = Timestamp(cx.services.now_ms() + secs * 1000);
    let (_, stmt) = cx
        .jobs
        .insert_unique(&key, op.chunk_kind(), &next, Some(at))?;
    cx.services.db.execute(stmt).await?;
    Ok(())
}
