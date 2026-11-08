//! The chunk Jobs of [`super::maint`]: `announcements.edit.chunk` and
//! `announcements.retract.chunk` each work through at most
//! [`super::maint::CHUNK`] delivery rows after the payload's cursor
//! `after`, and on `Error::RateLimited` enqueue their continuation.

use serde_json::{Value as Json, json};

use super::maint::{DELETE_WINDOW_MS, Op, changed, current, get_int, get_str, parse, wants_edit};
use super::store;
use crate::delivery::{ChannelAccountId, PlatformMessageId, Timestamp};
use crate::manifest::Job;
use crate::message::{Locale, Message};
use crate::module::JobContext;
use crate::port_types::{Rows, Statement};
use crate::{Error, Result};

/// Continuations one chunk may chain; past it the rows left count as
/// failed (an edit) or get `removal = 'failed'` (a retract), and the
/// chain ends.
pub(crate) const MAX_HOPS: i64 = 50;

/// The chunk's rows still to do, after the cursor `after`.
async fn chunk_rows(cx: &JobContext, id: &str, p: &Json, op: Op) -> Result<Rows> {
    let sql = format!(
        "SELECT x.id, ca.external_id, s.locale, x.sent_at, x.platform_message_id, x.acked_at \
         FROM (SELECT * FROM announcement_delivery WHERE announcement_id = ? AND channel = ? \
         ORDER BY id LIMIT ? OFFSET ?) x JOIN channel_account ca ON ca.id = x.channel_account_id \
         JOIN student s ON s.id = x.student_id WHERE x.status = 'sent' \
         AND x.platform_message_id IS NOT NULL AND x.id > ?{} ORDER BY x.id",
        op.todo()
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

/// One row to work on.
struct Item {
    delivery: String,
    to: ChannelAccountId,
    pid: PlatformMessageId,
    locale: Locale,
    acked: bool,
    sent_at: i64,
}

async fn chunk(cx: JobContext, job: Job, op: Op) -> Result<()> {
    let (p, id, version) = parse(&job)?;
    let Some(row) = current(&cx, op, &id, version).await? else {
        return Ok(());
    };
    let channel = get_str(&p, "channel");
    let Some(ch) = cx.deployment.channel(&channel) else {
        return Ok(());
    };
    let (editor, deleter) = (ch.editor(), ch.deleter());
    if (op == Op::Edit && editor.is_none()) || (op == Op::Retract && deleter.is_none()) {
        return Ok(());
    }
    let s = &cx.services;
    let rows = chunk_rows(&cx, &id, &p, op).await?;
    let texts = store::texts(s, &id).await?;
    let images = super::files::list(s, &id, super::files::IMAGE).await?;
    let changed = changed(&p);
    let mut items = Vec::with_capacity(rows.len());
    for i in 0..rows.len() {
        let own = store::text(&rows, i, "locale");
        if op == Op::Edit && !wants_edit(&texts, &changed, &own) {
            continue;
        }
        items.push(Item {
            delivery: store::text(&rows, i, "id"),
            to: ChannelAccountId::new(ch.kind(), store::text(&rows, i, "external_id")),
            pid: PlatformMessageId(store::text(&rows, i, "platform_message_id")),
            locale: texts.locale_for(&Locale::new(own.as_str())),
            acked: store::int(&rows, i, "acked_at").is_some(),
            sent_at: store::int(&rows, i, "sent_at").unwrap_or(0),
        });
    }
    let now = s.now_ms();
    // `ok` counts edits made (an edit) or rows with a removal set without
    // an error (a retract: deleted or too old).
    let (mut ok, mut failed) = (0i64, 0i64);
    let mut done = get_str(&p, "after");
    let mut limited: Option<(usize, Error)> = None;
    for (k, it) in items.iter().enumerate() {
        let result = match (op, editor, deleter) {
            (Op::Edit, Some(editor), _) => {
                let ack = row.require_ack && !it.acked;
                let d = &cx.deployment;
                let public = d.config().public_url.as_ref();
                let card =
                    super::chat::card(d.catalog(), &it.locale, &id, &texts, ack, &images, public)?;
                let msg = Message::Card(card);
                editor
                    .edit(&it.to, &it.pid, &msg, &it.locale)
                    .await
                    .map(|()| "edited")
            }
            (Op::Retract, _, Some(_)) if now - it.sent_at > DELETE_WINDOW_MS => Ok("too_old"),
            (Op::Retract, _, Some(deleter)) => {
                deleter.delete(&it.to, &it.pid).await.map(|()| "deleted")
            }
            _ => break,
        };
        let removal = match result {
            Err(e) if matches!(e.root(), Error::RateLimited { .. }) => {
                limited = Some((k, e));
                break;
            }
            Ok(r) => {
                ok += 1;
                r
            }
            Err(_) => {
                failed += 1;
                "failed"
            }
        };
        if op == Op::Retract {
            s.db.execute(set_removal(&it.delivery, removal)).await?;
        }
        done.clone_from(&it.delivery);
    }
    // The report and the continuation (or the end of the chain) in one
    // batch: a re-run never counts twice nor loses the continuation.
    let mut stmts = Vec::new();
    if let Some((k, e)) = limited {
        let hop = get_int(&p, "hop") + 1;
        if hop > MAX_HOPS {
            let left = &items[k..];
            failed += i64::try_from(left.len()).unwrap_or(0);
            if op == Op::Retract {
                stmts.extend(left.iter().map(|it| set_removal(&it.delivery, "failed")));
            }
        } else {
            stmts.push(continuation(&cx, op, &p, &done, hop, &e)?);
        }
    }
    if op == Op::Edit && ok + failed > 0 {
        stmts.push(add_to_report(&cx, &id, version, ok, failed));
    }
    if !stmts.is_empty() {
        s.db.batch(stmts).await?;
    }
    Ok(())
}

fn set_removal(delivery: &str, removal: &str) -> Statement {
    Statement::new("UPDATE announcement_delivery SET removal = ? WHERE id = ? AND removal IS NULL")
        .bind(removal.to_string())
        .bind(delivery.to_string())
}

/// Adds to `edit_report` while it is still this version's.
fn add_to_report(cx: &JobContext, id: &str, version: i64, ok: i64, failed: i64) -> Statement {
    Statement::new(
        "UPDATE announcement SET edit_report = json_set(edit_report, '$.at', ?, \
         '$.edited', json_extract(edit_report, '$.edited') + ?, \
         '$.failed', json_extract(edit_report, '$.failed') + ?) \
         WHERE id = ? AND json_extract(edit_report, '$.version') = ?",
    )
    .bind(cx.services.now_ms())
    .bind(ok)
    .bind(failed)
    .bind(id)
    .bind(version)
}

/// A rate limit stopped the chunk: its continuation resumes after `done`
/// (the last row handled), due after `retry_after_secs`, keyed
/// `<chunk key>:<hop>` so a re-run of this Job enqueues it once.
fn continuation(
    cx: &JobContext,
    op: Op,
    p: &Json,
    done: &str,
    hop: i64,
    e: &Error,
) -> Result<Statement> {
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
    Ok(cx
        .jobs
        .insert_unique(&key, op.chunk_kind(), &next, Some(at))?
        .1)
}
