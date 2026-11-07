//! The edit notice (#77). A Channel that cannot edit a sent message
//! (LINE: no `editor()`) keeps the old Card, so when Staff save an edit of
//! a sent announcement with "notify recipients of the edit" ticked, each
//! recipient there gets one short Push (a Card: "the announcement was
//! edited", its title, the "เปิดดูรายละเอียด" button to the Mini App page).
//! Channels with `editor()` (Telegram) get nothing extra: the edit Job
//! replaces their Card in place.
//!
//! The save writes `notice_report = {"version": v, …}` and enqueues
//! `announcements.notice` (key `announcements:<id>:notice:v<v>`), in the
//! same gated batch. That Job counts the recipients once (audit
//! `announcement.notice` with the count) and fans out one
//! `announcements.notice.chunk` per [`CHUNK`] delivery rows per Channel,
//! [`PER_MINUTE`] a minute, like the edit and retract chunks
//! ([`super::maint`]).
//!
//! Recipients are the delivery rows: per Link the newest one `sent`
//! (hand-picked Students included, a resend not counted twice), while the
//! Link is still `active` and the account has not blocked the bot. Each
//! Push is keyed `announcements:<id>:notice:v<v>:<delivery id>` and
//! metered like any Push; the row's `notice_version` is then set, so a
//! re-run notifies no one twice. A later save that wants a notice
//! overwrites `notice_report` and the older run stops before its next
//! Push; a retract stops it too. A rate limit enqueues the chunk's
//! continuation, as the edit chunks do.

use serde_json::{Value as Json, json};

use super::maint::{CHUNK, PER_MINUTE, get_int, get_str, parse};
use super::maint_chunk::MAX_HOPS;
use super::store::{self, Row};
use super::{NAME, NOTICE_CHUNK};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::delivery::{ChannelAccountId, Timestamp};
use crate::manifest::Job;
use crate::message::{Button, Card, Locale, Message};
use crate::module::JobContext;
use crate::port_types::Statement;
use crate::{Error, Result};

/// The notice Job's key for version `version`.
pub(crate) fn notice_key(id: &str, version: i64) -> String {
    format!("{NAME}:{id}:notice:v{version}")
}

/// The delivery rows (alias `x`) still to notify of version `?`.
const TARGET: &str = "x.status = 'sent' AND coalesce(x.notice_version, -1) < ? \
    AND NOT EXISTS (SELECT 1 FROM announcement_delivery y WHERE y.announcement_id = \
    x.announcement_id AND y.link_id = x.link_id AND y.status = 'sent' AND y.id > x.id) \
    AND EXISTS (SELECT 1 FROM link l WHERE l.id = x.link_id AND l.status = 'active') \
    AND NOT EXISTS (SELECT 1 FROM channel_account c WHERE c.id = x.channel_account_id \
    AND c.blocked_at IS NOT NULL)";

/// The Channel gets the notice: it exists and cannot edit.
fn wants(cx: &JobContext, channel: &str) -> bool {
    cx.deployment
        .channel(channel)
        .is_some_and(|c| c.editor().is_none())
}

/// The announcement while notice `version` still applies: `sent` (or
/// `sending` again, a resend) and `notice_report` still this version's.
async fn current(cx: &JobContext, id: &str, version: i64) -> Result<Option<Row>> {
    Ok(store::load(&cx.services, id).await?.filter(|r| {
        matches!(r.status.as_str(), "sent" | "sending")
            && r.notice_report
                .as_ref()
                .and_then(|n| n.get("version"))
                .and_then(Json::as_i64)
                == Some(version)
    }))
}

/// `announcements.notice`: counts the recipients (the first run only:
/// `total` is still null; audit `announcement.notice` with the count, as
/// the Job) and fans out the chunks.
pub(crate) async fn notice(cx: JobContext, job: Job) -> Result<()> {
    let (_, id, version) = parse(&job)?;
    let s = &cx.services;
    let Some(row) = current(&cx, &id, version).await? else {
        return Ok(());
    };
    let mut stmts = Vec::new();
    let counted = row
        .notice_report
        .as_ref()
        .and_then(|n| n.get("total"))
        .is_some_and(|t| !t.is_null());
    if !counted {
        let total = total(&cx, &id, version).await?;
        let first = Statement::new(
            "EXISTS (SELECT 1 FROM announcement WHERE id = ? \
             AND json_extract(notice_report, '$.version') = ? \
             AND json_extract(notice_report, '$.total') IS NULL)",
        )
        .bind(id.as_str())
        .bind(version);
        let (entry, _) = audit::entry_if(
            s,
            &Actor::Job(job.id.clone()),
            "announcement.notice",
            Some(&Target::new(NAME, &id)),
            None,
            Some(json!({ "version": version, "recipients": total })),
            &RequestCtx::default(),
            first,
        )?;
        stmts.push(entry);
        stmts.push(
            Statement::new(
                "UPDATE announcement SET notice_report = json_set(notice_report, \
                 '$.total', ?, '$.at', ?) WHERE id = ? \
                 AND json_extract(notice_report, '$.version') = ? \
                 AND json_extract(notice_report, '$.total') IS NULL",
            )
            .bind(total)
            .bind(s.now_ms())
            .bind(id.as_str())
            .bind(version),
        );
    }
    stmts.extend(fan_out(&cx, &id, version).await?);
    s.db.batch(stmts).await?;
    Ok(())
}

/// Recipients on Channels that get the notice.
async fn total(cx: &JobContext, id: &str, version: i64) -> Result<i64> {
    let sql = format!(
        "SELECT x.channel, count(*) AS n FROM announcement_delivery x \
         WHERE x.announcement_id = ? AND {TARGET} GROUP BY x.channel"
    );
    let rows = cx
        .services
        .db
        .query(Statement::new(sql).bind(id).bind(version))
        .await?;
    Ok((0..rows.len())
        .filter(|&i| wants(cx, &store::text(&rows, i, "channel")))
        .map(|i| store::int(&rows, i, "n").unwrap_or(0))
        .sum())
}

/// One chunk Job per [`CHUNK`] delivery rows (all rows, in id order) per
/// Channel that gets the notice, for the chunks with a recipient left;
/// keys `<notice key>:<channel>:<n>`, so a re-run inserts nothing.
async fn fan_out(cx: &JobContext, id: &str, version: i64) -> Result<Vec<Statement>> {
    let s = &cx.services;
    let sql = format!(
        "SELECT channel, n FROM (SELECT x.*, (row_number() OVER \
         (PARTITION BY x.channel ORDER BY x.id) - 1) / ? AS n FROM announcement_delivery x \
         WHERE x.announcement_id = ?) x WHERE {TARGET} GROUP BY channel, n ORDER BY channel, n"
    );
    let size = i64::try_from(CHUNK).unwrap_or(25);
    let rows =
        s.db.query(Statement::new(sql).bind(size).bind(id).bind(version))
            .await?;
    let now = s.now_ms();
    let base = notice_key(id, version);
    let (mut stmts, mut paced, mut last) = (Vec::new(), 0usize, String::new());
    for i in 0..rows.len() {
        let channel = store::text(&rows, i, "channel");
        if !wants(cx, &channel) {
            continue;
        }
        if channel != last {
            paced = 0;
            last.clone_from(&channel);
        }
        let n = store::int(&rows, i, "n").unwrap_or(0);
        let key = format!("{base}:{channel}:{n}");
        let minute = i64::try_from(paced / PER_MINUTE).unwrap_or(0);
        paced += 1;
        let payload = json!({
            "id": id, "version": version, "channel": channel,
            "offset": n * size, "size": size, "key": key, "hop": 0,
        });
        let at = Timestamp(now + minute * 60_000);
        stmts.push(
            cx.jobs
                .insert_unique(&key, NOTICE_CHUNK, &payload, Some(at))?
                .1,
        );
    }
    Ok(stmts)
}

/// The notice Card in `locale`: what happened, the title, the button.
fn card(cx: &JobContext, locale: &Locale, id: &str, texts: &store::Texts) -> Card {
    let catalog = &cx.catalog;
    Card {
        title: catalog.t(locale, "announcements.notice.title"),
        body: texts.pick(locale).0.to_string(),
        buttons: vec![Button::mini_app(
            catalog.t(locale, "announcements.card.open"),
            super::chat::detail_path(id),
        )],
    }
}

/// `announcements.notice.chunk`: one keyed Push per recipient of the
/// chunk not yet notified of this version, each followed by its row's
/// `notice_version` and the report's count in one batch.
pub(crate) async fn notice_chunk(cx: JobContext, job: Job) -> Result<()> {
    let (p, id, version) = parse(&job)?;
    let channel = get_str(&p, "channel");
    if current(&cx, &id, version).await?.is_none() || !wants(&cx, &channel) {
        return Ok(());
    }
    let s = &cx.services;
    let Some(kind) = cx.deployment.channel(&channel).map(|c| c.kind()) else {
        return Ok(());
    };
    let sql = format!(
        "SELECT x.id, ca.external_id, s.locale FROM (SELECT * FROM announcement_delivery \
         WHERE announcement_id = ? AND channel = ? ORDER BY id LIMIT ? OFFSET ?) x \
         JOIN channel_account ca ON ca.id = x.channel_account_id \
         JOIN student s ON s.id = x.student_id WHERE {TARGET} ORDER BY x.id"
    );
    let st = Statement::new(sql)
        .bind(id.as_str())
        .bind(channel.as_str())
        .bind(get_int(&p, "size"))
        .bind(get_int(&p, "offset"))
        .bind(version);
    let rows = s.db.query(st).await?;
    let texts = store::texts(s, &id).await?;
    for i in 0..rows.len() {
        // Re-read before each Push: a later notice or a retract stops it.
        if current(&cx, &id, version).await?.is_none() {
            return Ok(());
        }
        let delivery = store::text(&rows, i, "id");
        let to = ChannelAccountId::new(kind, store::text(&rows, i, "external_id"));
        let locale = texts.locale_for(&Locale::new(store::text(&rows, i, "locale")));
        let msgs = [Message::Card(card(&cx, &locale, &id, &texts))];
        let key = format!("{}:{delivery}", notice_key(&id, version));
        let sent = cx
            .deployment
            .send_many_with_key(&channel, &[to], &msgs, &locale, Some(NAME), &key)
            .await?;
        let result = match (sent.results.into_iter().next(), sent.stopped) {
            (Some(r), _) => r.map(|_| ()),
            (None, Some(e)) => Err(e),
            (None, None) => Err(Error::Platform("no result".into())),
        };
        let ok = match result {
            Err(e) if matches!(e.root(), Error::RateLimited { .. }) => {
                return limited(&cx, &p, &id, version, &rows, i, &e).await;
            }
            r => r.is_ok(),
        };
        s.db.batch(vec![
            mark(&delivery, version),
            report(&cx, &id, version, ok, 1),
        ])
        .await?;
    }
    Ok(())
}

/// The row is done with this version.
fn mark(delivery: &str, version: i64) -> Statement {
    Statement::new("UPDATE announcement_delivery SET notice_version = ? WHERE id = ?")
        .bind(version)
        .bind(delivery.to_string())
}

/// Adds `n` to `sent` (`ok`) or `failed` while the report is this version's.
fn report(cx: &JobContext, id: &str, version: i64, ok: bool, n: i64) -> Statement {
    let field = if ok { "sent" } else { "failed" };
    Statement::new(format!(
        "UPDATE announcement SET notice_report = json_set(notice_report, '$.at', ?, \
         '$.{field}', coalesce(json_extract(notice_report, '$.{field}'), 0) + ?) \
         WHERE id = ? AND json_extract(notice_report, '$.version') = ?"
    ))
    .bind(cx.services.now_ms())
    .bind(n)
    .bind(id.to_string())
    .bind(version)
}

/// A rate limit stopped the chunk at row `at`: its continuation (same
/// rows; those notified are filtered out), due after `retry_after_secs`
/// and keyed `<chunk key>:<hop>`; past [`MAX_HOPS`] the rows left count
/// as failed.
async fn limited(
    cx: &JobContext,
    p: &Json,
    id: &str,
    version: i64,
    rows: &crate::port_types::Rows,
    at: usize,
    e: &Error,
) -> Result<()> {
    let hop = get_int(p, "hop") + 1;
    let mut stmts = Vec::new();
    if hop > MAX_HOPS {
        for i in at..rows.len() {
            stmts.push(mark(&store::text(rows, i, "id"), version));
        }
        let left = i64::try_from(rows.len() - at).unwrap_or(0);
        stmts.push(report(cx, id, version, false, left));
    } else {
        let secs = match e.root() {
            Error::RateLimited { retry_after_secs } => i64::from(*retry_after_secs).max(1),
            _ => 1,
        };
        let mut next = p.clone();
        if let Some(o) = next.as_object_mut() {
            o.insert("hop".into(), json!(hop));
        }
        let key = format!("{}:{hop}", get_str(p, "key"));
        let run_at = Timestamp(cx.services.now_ms() + secs * 1000);
        stmts.push(
            cx.jobs
                .insert_unique(&key, NOTICE_CHUNK, &next, Some(run_at))?
                .1,
        );
    }
    cx.services.db.batch(stmts).await?;
    Ok(())
}
