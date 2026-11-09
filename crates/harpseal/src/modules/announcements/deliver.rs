//! Sending (#10, #25): the parent Job `announcements.deliver` snapshots
//! the Audience into `announcement_delivery` rows for its round and
//! enqueues one `announcements.deliver.channel` Job per Channel per chunk
//! (a multicast group, or 25 accounts paced on Channels without one); the
//! child that leaves no `pending` row of the round marks it `sent`.

use std::collections::BTreeMap;

use serde_json::{Value as Json, json};

use super::gate::JobReq;
use super::outcome::{failed, is_final, kind_of, max_retries, outcome, skipped, skipped_channel};
use super::store::{self, Row};
use super::{DELIVER, DELIVER_CHANNEL, NAME};
use crate::delivery::{ChannelAccountId, Timestamp};
use crate::manifest::Job;
use crate::message::Locale;
use crate::module::JobContext;
use crate::port_types::Statement;
use crate::{Deployment, Error, Result, Services, audience};

/// Accounts per child Job on a Channel without multicast.
pub(crate) const PACED_CHUNK: usize = 25;
/// Paced chunks started per minute (25 × 10 = 250 messages a minute).
const PACED_PER_MINUTE: usize = 10;
/// Rows per `Db::batch` when snapshotting.
const SNAPSHOT_BATCH: usize = 400;

/// The parent Job's unique key: per round and version, so a resend or a
/// schedule confirmed again never reuses an earlier key.
pub(crate) fn deliver_key(id: &str, round: i64, version: i64) -> String {
    format!("{NAME}:{id}:{round}:v{version}")
}

/// The parent Job's payload.
pub(crate) fn payload(id: &str, round: i64, version: i64) -> Json {
    json!({ "id": id, "round": round, "version": version })
}

fn field<'a>(p: &'a Json, k: &str) -> Result<&'a Json> {
    p.get(k)
        .ok_or_else(|| Error::Invalid(format!("job payload has no `{k}`")))
}

fn parse(job: &Job) -> Result<(Json, String, i64)> {
    let p: Json = serde_json::from_str(&job.payload)
        .map_err(|_| Error::Invalid("job payload is not JSON".into()))?;
    let id = field(&p, "id")?.as_str().unwrap_or_default().to_string();
    let round = field(&p, "round")?.as_i64().unwrap_or(-1);
    Ok((p, id, round))
}

/// `announcements.deliver`.
pub(crate) async fn parent(cx: JobContext, job: Job) -> Result<()> {
    let (p, id, round) = parse(&job)?;
    let version = field(&p, "version")?.as_i64().unwrap_or(-1);
    let d = cx.deployment.as_ref();
    let s = d.services();
    let claim =
        s.db.execute(
            Statement::new(
                "UPDATE announcement SET status = 'sending', updated_at = ? WHERE id = ? \
                 AND round = ? AND version = ? AND status IN ('scheduled', 'sending')",
            )
            .bind(s.now_ms())
            .bind(id.as_str())
            .bind(round)
            .bind(version),
        )
        .await?;
    if claim.rows_affected == 0 {
        // Cancelled, edited (a newer Job holds the newer version),
        // retracted or already resent: nothing to do.
        return Ok(());
    }
    let Some(row) = store::load(s, &id).await? else {
        return Ok(());
    };
    if row.snapshot_round < round {
        snapshot(d, &row).await?;
    }
    enqueue_children(&cx, d, &row).await?;
    finish_if_done(s, &id, round).await
}

/// One `pending` row per live Link of the Audience (`skipped` for
/// unreachable ones), then `snapshot_round = round`. Re-running inserts
/// nothing twice (UNIQUE (announcement, round, link)).
async fn snapshot(d: &Deployment, row: &Row) -> Result<()> {
    let s = d.services();
    let now = s.now_ms();
    let recipients = audience::resolve(d, &row.audience).await?;
    let mut stmts = Vec::with_capacity(recipients.len());
    for r in &recipients {
        let (status, error) = if r.unreachable {
            ("skipped", Some("unreachable"))
        } else {
            ("pending", None)
        };
        stmts.push(
            Statement::new(
                "INSERT INTO announcement_delivery (id, announcement_id, round, student_id, \
                 link_id, channel, channel_account_id, status, error, acked_at) VALUES \
                 (?, ?, ?, ?, ?, ?, ?, ?, ?, (SELECT max(acked_at) FROM announcement_delivery \
                 WHERE announcement_id = ? AND student_id = ?)) \
                 ON CONFLICT (announcement_id, round, link_id) DO NOTHING",
            )
            .bind(s.new_id()?)
            .bind(row.id.as_str())
            .bind(row.round)
            .bind(r.student_id.as_str())
            .bind(r.link_id.as_str())
            .bind(r.account.channel.as_str())
            .bind(r.channel_account_id.as_str())
            .bind(status)
            .bind(error)
            // An acknowledgement is once per Student: a resent round keeps it.
            .bind(row.id.as_str())
            .bind(r.student_id.as_str()),
        );
    }
    for chunk in stmts.chunks(SNAPSHOT_BATCH) {
        s.db.batch(chunk.to_vec()).await?;
    }
    s.db.execute(
        Statement::new(
            "UPDATE announcement SET snapshot_round = ?, updated_at = ? WHERE id = ? \
             AND snapshot_round < ?",
        )
        .bind(row.round)
        .bind(now)
        .bind(row.id.as_str())
        .bind(row.round),
    )
    .await?;
    Ok(())
}

/// Chunk size on `channel`: the multicast group, else [`PACED_CHUNK`].
fn chunk_size(d: &Deployment, channel: &str) -> Option<(usize, bool)> {
    let ch = d.channel(channel)?;
    Some(match ch.multicast() {
        Some(m) => (m.max_recipients().max(1), false),
        None => (PACED_CHUNK, true),
    })
}

async fn enqueue_children(cx: &JobContext, d: &Deployment, row: &Row) -> Result<()> {
    let s = d.services();
    let rows =
        s.db.query(
            Statement::new(
                "SELECT channel, count(*) AS n FROM announcement_delivery WHERE \
                 announcement_id = ? AND round = ? GROUP BY channel ORDER BY channel",
            )
            .bind(row.id.as_str())
            .bind(row.round),
        )
        .await?;
    let now = s.now_ms();
    let mut stmts = Vec::new();
    for i in 0..rows.len() {
        let channel = store::text(&rows, i, "channel");
        let total = usize::try_from(store::int(&rows, i, "n").unwrap_or(0)).unwrap_or(0);
        let Some((size, paced)) = chunk_size(d, &channel) else {
            stmts.push(skipped_channel(row.id.as_str(), row.round, &channel));
            continue;
        };
        for n in 0..total.div_ceil(size) {
            let key = format!("{NAME}:{}:{}:{channel}:{n}", row.id, row.round);
            let minute = if paced { n / PACED_PER_MINUTE } else { 0 };
            let at = Timestamp(now + i64::try_from(minute).unwrap_or(0) * 60_000);
            let payload = json!({
                "id": row.id, "round": row.round, "channel": channel,
                "offset": n * size, "size": size,
            });
            let (_, stmt) = cx
                .jobs
                .insert_unique(&key, DELIVER_CHANNEL, &payload, Some(at))?;
            stmts.push(stmt);
        }
    }
    if !stmts.is_empty() {
        s.db.batch(stmts).await?;
    }
    Ok(())
}

/// Marks round `round` `sent` once none of its rows is `pending`.
pub(crate) async fn finish_if_done(s: &Services, id: &str, round: i64) -> Result<()> {
    let now = s.now_ms();
    s.db.execute(
        Statement::new(
            "UPDATE announcement SET status = 'sent', sent_at = ?, updated_at = ? WHERE id = ? \
             AND round = ? AND status = 'sending' AND snapshot_round = round \
             AND NOT EXISTS (SELECT 1 FROM announcement_delivery WHERE announcement_id = ? \
             AND round = ? AND status = 'pending')",
        )
        .bind(now)
        .bind(now)
        .bind(id)
        .bind(round)
        .bind(id)
        .bind(round),
    )
    .await?;
    Ok(())
}

/// Round `round` of `id` is still the one being sent.
async fn still_sending(s: &Services, id: &str, round: i64) -> Result<bool> {
    let rows =
        s.db.query(
            Statement::new(
                "SELECT 1 AS x FROM announcement WHERE id = ? AND round = ? AND status = 'sending'",
            )
            .bind(id)
            .bind(round),
        )
        .await?;
    Ok(!rows.is_empty())
}

/// One pending delivery of a chunk.
pub(super) struct Pending {
    pub(super) id: String,
    pub(super) account: ChannelAccountId,
}

/// `announcements.deliver.channel`: sends one chunk, a Card per Locale
/// group, and records each delivery's outcome.
pub(crate) async fn child(cx: JobContext, job: Job) -> Result<()> {
    let (p, id, round) = parse(&job)?;
    let channel = field(&p, "channel")?
        .as_str()
        .unwrap_or_default()
        .to_string();
    let offset = field(&p, "offset")?.as_i64().unwrap_or(0);
    let size = field(&p, "size")?.as_i64().unwrap_or(0);
    let d = cx.deployment.as_ref();
    let s = d.services();
    let Some(row) = store::load(s, &id).await? else {
        return Ok(());
    };
    if row.status != "sending" || row.round != round {
        return Ok(());
    }
    let Some(kind) = d.channel(&channel).map(|c| c.kind()) else {
        s.db.execute(skipped_channel(&id, round, &channel)).await?;
        return finish_if_done(s, &id, round).await;
    };
    let rows =
        s.db.query(
            Statement::new(
                "SELECT x.id, x.status, x.acked_at, ca.external_id, ca.blocked_at, s.locale \
                 FROM (SELECT * FROM \
                 announcement_delivery WHERE announcement_id = ? AND round = ? AND channel = ? \
                 ORDER BY id LIMIT ? OFFSET ?) x JOIN channel_account ca \
                 ON ca.id = x.channel_account_id JOIN student s ON s.id = x.student_id \
                 WHERE x.status = 'pending' ORDER BY x.id",
            )
            .bind(id.as_str())
            .bind(round)
            .bind(channel.as_str())
            .bind(size)
            .bind(offset),
        )
        .await?;
    let texts = store::texts(s, &id).await?;
    let images = super::files::list(s, &id).await?;
    // One Card per (Locale, already acknowledged): no [รับทราบ] for those.
    let mut groups: BTreeMap<(Locale, bool), Vec<Pending>> = BTreeMap::new();
    let mut blocked = Vec::new();
    for i in 0..rows.len() {
        let delivery = store::text(&rows, i, "id");
        if store::int(&rows, i, "blocked_at").is_some() {
            // Blocked since the snapshot: never sent, never metered.
            blocked.push(skipped(&delivery));
            continue;
        }
        let locale = texts.locale_for(&Locale::new(store::text(&rows, i, "locale")));
        let acked = store::int(&rows, i, "acked_at").is_some();
        groups.entry((locale, acked)).or_default().push(Pending {
            id: delivery,
            account: ChannelAccountId::new(kind, store::text(&rows, i, "external_id")),
        });
    }
    if !blocked.is_empty() {
        s.db.batch(blocked).await?;
    }
    let last = job.attempt > max_retries(s, &job.id).await?;
    let mut again: Option<Error> = None;
    for ((locale, acked), group) in groups {
        // Re-read right before the send: a retract (or a resend) since
        // this run started leaves the rest unsent (#49).
        if !still_sending(s, &id, round).await? {
            break;
        }
        let ack = row.require_ack && !acked;
        let public = d.config().public_url.as_ref();
        let card = super::chat::card(d.catalog(), &locale, &id, &texts, ack, &images, public)?;
        let to: Vec<ChannelAccountId> = group.iter().map(|g| g.account.clone()).collect();
        let key = format!(
            "{}/{channel}/{offset}/{}/{}",
            job.id,
            locale.as_str(),
            u8::from(acked)
        );
        let msgs = [crate::message::Message::Card(card)];
        let sent = d
            .send_many_with_key(&channel, &to, &msgs, &locale, Some(NAME), &key)
            .await?;
        let mut stmts = Vec::new();
        let now = s.now_ms();
        for (g, result) in group.iter().zip(&sent.results) {
            stmts.push(outcome(g, result, now, last));
            if let Err(e) = result
                && !last
                && !is_final(e)
            {
                again.get_or_insert_with(|| Error::Platform(kind_of(e).into()));
            }
        }
        if last {
            let kind = sent.stopped.as_ref().map_or("platform", kind_of);
            for g in group.iter().skip(sent.results.len()) {
                stmts.push(failed(&g.id, kind));
            }
        }
        if !stmts.is_empty() {
            s.db.batch(stmts).await?;
        }
        if let Some(e) = sent.stopped {
            again.get_or_insert(e);
        }
    }
    finish_if_done(s, &id, round).await?;
    match again {
        Some(e) if !last => Err(e),
        _ => Ok(()),
    }
}

/// The parent Job of round `round` at `version` (inserted by
/// [`super::gate::cas`], gated on the write's audit row).
pub(crate) fn parent_job(id: &str, round: i64, version: i64, run_at: Option<i64>) -> JobReq {
    JobReq {
        key: deliver_key(id, round, version),
        kind: DELIVER,
        payload: payload(id, round, version),
        run_at,
    }
}
