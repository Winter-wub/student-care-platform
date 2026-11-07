//! The Kernel send path: one batch per reply token, Capability checks
//! before anything goes out, and one `outbound_message` row plus a
//! `usage_daily` upsert (its metered units) per Message, written in a
//! single `Db::batch`. A batch the Channel stops part-way is recorded as
//! far as it went and reported as `Error::PartialSend`.
//!
//! `usage_daily.failed` counts failed attempts to the platform, not failed
//! Messages: a Message that fails twice and goes out on the third try
//! counts 2 there (and 1 in `metered`). `Error::RateLimited` adds nothing
//! (the caller retries later), and neither does a Push skipped before the
//! platform because the account blocked the bot.

use super::Deployment;
use super::day::civil_day;
use crate::delivery::{
    BatchReceipt, Capability, ChannelAccountId, Delivery, ReplyToken, SendReceipt,
};
use crate::message::{Locale, Message};
use crate::port_types::Statement;
use crate::{Error, Result};

/// The Capability a Message needs beyond the Channel minimum.
pub(super) fn required(msg: &Message) -> Option<Capability> {
    match msg {
        Message::Attachment(_) => Some(Capability::SendAttachment),
        _ => None,
    }
}

impl Deployment {
    /// Picks Reply or Push ([`Delivery::select`]) and sends one message,
    /// recording it like [`Deployment::send_batch`].
    pub async fn send(
        &self,
        to: &ChannelAccountId,
        reply: Option<&ReplyToken>,
        msg: &Message,
        locale: &Locale,
    ) -> Result<SendReceipt> {
        let mut receipts = self
            .send_batch(to, reply, std::slice::from_ref(msg), locale)
            .await?;
        receipts
            .pop()
            .ok_or_else(|| Error::Platform("the channel returned no receipt".into()))
    }

    /// Sends `msgs` to one account as one batch: [`Delivery::select`]
    /// decides Reply or Push once for the whole batch, and the Channel
    /// spends the reply token as far as it reaches (on LINE, the first 5
    /// objects; the rest go as metered Pushes).
    ///
    /// Before anything is sent, a Message needing a Capability the Channel
    /// lacks fails the whole batch with `Error::Unsupported` and nothing is
    /// recorded. After the send, every Message sent gets one
    /// `outbound_message` row and its metered units
    /// ([`SendReceipt::metered_units`]) count in `usage_daily (channel,
    /// day)`, all in one `Db::batch`. When the Channel returns an error,
    /// every Message that did not wholly go out (the one it stopped in
    /// included) adds 1 to `usage_daily.failed` (#26), in the same batch
    /// when something was sent, else on its own (best effort). A Push to an
    /// account that blocked the bot is skipped with `Error::Unreachable`
    /// before the Channel is called and counts nowhere; a send the Channel refuses
    /// with `Error::Unreachable` marks the account's Link `unreachable`.
    ///
    /// If the Channel stops part-way, what went out is recorded (the
    /// Message it stopped in gets a row with `error` set when some of its
    /// parts went out) and the result is [`Error::PartialSend`]: resume
    /// with `msgs[sent..]`, ideally through
    /// [`Deployment::send_batch_with_key`] with the same key. When the
    /// stopped Message opened its own Push requests, the resume rebuilds
    /// them with the same retry keys: those already accepted (LINE answers
    /// 409, [`SendReceipt::replayed_units`]) are not delivered or metered
    /// again, and its "partly sent" row is completed rather than a second
    /// one written. When the stopped Message shared its first request with
    /// the Messages before it, the resume builds different requests, LINE
    /// answers no 409 and those parts reach the Student twice (#62). When nothing
    /// went out the Channel's error is returned as is. If the write fails,
    /// a database error is returned (the Messages did go out); it names the
    /// Channel's error too when there was one.
    pub async fn send_batch(
        &self,
        to: &ChannelAccountId,
        reply: Option<&ReplyToken>,
        msgs: &[Message],
        locale: &Locale,
    ) -> Result<Vec<SendReceipt>> {
        self.deliver_keyed(to, reply, msgs, locale, None, None)
            .await
    }

    /// [`Deployment::send_batch`] with an idempotency `key` for the batch
    /// (e.g. derived from a Job id and the recipient). A retry that passes
    /// the same key, account and Messages lets the Channel de-duplicate
    /// requests that already went out (LINE `X-Line-Retry-Key`, honoured
    /// by LINE for 24 hours). Without a key each call gets a fresh one.
    pub async fn send_batch_with_key(
        &self,
        to: &ChannelAccountId,
        reply: Option<&ReplyToken>,
        msgs: &[Message],
        locale: &Locale,
        key: &str,
    ) -> Result<Vec<SendReceipt>> {
        self.deliver_keyed(to, reply, msgs, locale, None, Some(key))
            .await
    }

    /// [`Deployment::send_batch`] on behalf of `module` (stored on the rows).
    pub(crate) async fn deliver(
        &self,
        to: &ChannelAccountId,
        reply: Option<&ReplyToken>,
        msgs: &[Message],
        locale: &Locale,
        module: Option<&str>,
    ) -> Result<Vec<SendReceipt>> {
        self.deliver_keyed(to, reply, msgs, locale, module, None)
            .await
    }

    /// A keyed Push on behalf of `module` ([`Deployment::send_many_with_key`]).
    pub(super) async fn deliver_with(
        &self,
        to: &ChannelAccountId,
        msgs: &[Message],
        locale: &Locale,
        module: Option<&str>,
        key: &str,
    ) -> Result<Vec<SendReceipt>> {
        self.deliver_keyed(to, None, msgs, locale, module, Some(key))
            .await
    }

    async fn deliver_keyed(
        &self,
        to: &ChannelAccountId,
        reply: Option<&ReplyToken>,
        msgs: &[Message],
        locale: &Locale,
        module: Option<&str>,
        key: Option<&str>,
    ) -> Result<Vec<SendReceipt>> {
        let channel = self.channel_or_err(to.channel.as_str())?;
        let caps = channel.capabilities();
        if let Some(c) = msgs.iter().filter_map(required).find(|c| !caps.has(*c)) {
            return Err(Error::Unsupported(c));
        }
        if msgs.is_empty() {
            return Ok(Vec::new());
        }
        let delivery = Delivery::select(reply, self.services.clock.now());
        if matches!(delivery, Delivery::Push) && self.blocked(to).await? {
            return Err(Error::Unreachable);
        }
        let key = match key {
            Some(k) => k.to_string(),
            None => self.services.new_id()?,
        };
        let batch = channel
            .send_batch_keyed(to, &delivery, msgs, locale, &key)
            .await;
        // Messages that reached the platform and did not wholly go out
        // (the one the batch stopped in counts: the Student did not get all
        // of it) count in `usage_daily.failed` (#26), per attempt. A 429 is
        // "try later", not a failure: the Job retries it.
        let failed = match &batch.error {
            Some(e) if !matches!(e.root(), Error::RateLimited { .. }) => {
                msgs.len().saturating_sub(batch.receipts.len())
            }
            _ => 0,
        };
        if batch.anything_sent() {
            let mut stmts = self.outbound_rows(to, &delivery, msgs, &batch, module, &key)?;
            if failed > 0 {
                stmts.push(self.failed_usage(to.channel.as_str(), failed));
            }
            if let Err(db) = self.services.db.batch(stmts).await {
                return Err(match &batch.error {
                    Some(e) => Error::Db(format!("{db}; the channel had stopped with: {e}")),
                    None => db,
                });
            }
        } else if failed > 0 {
            // Best effort: the Channel's error is what the caller needs.
            let stmt = self.failed_usage(to.channel.as_str(), failed);
            let _ = self.services.db.execute(stmt).await;
        }
        let Some(error) = batch.error else {
            return Ok(batch.receipts);
        };
        if let Error::Unreachable = error.root() {
            // The account blocked the bot: its Link becomes unreachable and
            // later sends skip it. The send error is what the caller sees.
            let stmts = crate::kernel::flow::mark_unreachable(&self.services, to);
            let _ = self.services.db.batch(stmts).await;
        }
        if batch.receipts.is_empty() && batch.unfinished.is_none() {
            return Err(error);
        }
        Err(Error::PartialSend {
            sent: batch.receipts.len(),
            total: msgs.len(),
            source: Box::new(error),
        })
    }

    /// The account blocked the bot (`blocked_at` set by `Inbound::Blocked`
    /// or an `Unreachable` send, cleared by its next Inbound): Pushes skip
    /// it and fail with `Error::Unreachable` without calling the Channel.
    async fn blocked(&self, to: &ChannelAccountId) -> Result<bool> {
        let rows = self
            .services
            .db
            .query(
                Statement::new(
                    "SELECT 1 FROM channel_account WHERE channel = ? AND external_id = ? \
                     AND blocked_at IS NOT NULL",
                )
                .bind(to.channel.as_str())
                .bind(to.external_id.clone()),
            )
            .await?;
        Ok(!rows.rows.is_empty())
    }

    /// Adds `n` failed Messages to today's `usage_daily` row of `channel`.
    pub(super) fn failed_usage(&self, channel: &str, n: usize) -> Statement {
        let offset = self.config.utc_offset_minutes().unwrap_or(0);
        Statement::new(
            "INSERT INTO usage_daily (channel, day, failed) VALUES (?, ?, ?) \
             ON CONFLICT (channel, day) DO UPDATE SET failed = failed + excluded.failed",
        )
        .bind(channel)
        .bind(civil_day(self.services.now_ms(), offset))
        .bind(i64::try_from(n).unwrap_or(i64::MAX))
    }

    /// Ensures the `channel_account` row, then per Message sent its usage
    /// upsert and `outbound_message` row (the Message the batch stopped in,
    /// when some of its parts went out, gets a row with `error` set). One
    /// `Db::batch`: all commit or none do.
    fn outbound_rows(
        &self,
        to: &ChannelAccountId,
        delivery: &Delivery,
        msgs: &[Message],
        batch: &BatchReceipt,
        module: Option<&str>,
        key: &str,
    ) -> Result<Vec<Statement>> {
        let now = self.services.now_ms();
        let channel = to.channel.as_str();
        let offset = self.config.utc_offset_minutes().unwrap_or(0);
        let day = civil_day(now, offset);
        let mut stmts = vec![
            Statement::new(
                "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?) ON CONFLICT (channel, external_id) DO NOTHING",
            )
            .bind(self.services.new_id()?)
            .bind(channel)
            .bind(to.external_id.clone())
            .bind(now)
            .bind(now),
        ];
        let unfinished_error = batch.error.as_ref().map(|e| {
            let text: String = e.to_string().chars().take(200).collect();
            format!("partly sent: {text}")
        });
        let rows = batch.receipts.iter().map(|r| (r, None)).chain(
            batch
                .unfinished
                .iter()
                .map(|r| (r, unfinished_error.clone())),
        );
        for (i, ((msg, r), error)) in msgs
            .iter()
            .zip(rows)
            .map(|(m, (r, e))| ((m, r), e))
            .enumerate()
        {
            let units = units_to_meter(i, r);
            stmts.push(
                Statement::new(
                    "INSERT INTO usage_daily (channel, day, metered, free) VALUES (?, ?, ?, ?) \
                     ON CONFLICT (channel, day) DO UPDATE SET \
                     metered = metered + excluded.metered, free = free + excluded.free",
                )
                .bind(channel)
                .bind(day.clone())
                .bind(i64::from(units))
                .bind(i64::from(units == 0)),
            );
            let resumed = is_resumed_part(i, r);
            // On the resume of the Message the last attempt stopped in, its
            // row of the same batch key (error "partly sent") is completed
            // instead of a second one inserted; the insert runs only when
            // that row cannot be found.
            let insert = if resumed {
                format!(
                    "INSERT INTO outbound_message (id, channel, channel_account_id, push, \
                     metering, platform_message_id, module, kind, sent_at, error, batch_key) \
                     SELECT ?, ?, {ACCOUNT}, ?, ?, ?, ?, ?, ?, ?, ? WHERE NOT EXISTS ({PARTIAL_ROW})"
                )
            } else {
                format!(
                    "INSERT INTO outbound_message (id, channel, channel_account_id, push, \
                     metering, platform_message_id, module, kind, sent_at, error, batch_key) \
                     VALUES (?, ?, {ACCOUNT}, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
            };
            let pushed = r.pushed || matches!(delivery, Delivery::Push);
            let metering = if units > 0 { "metered" } else { "free" };
            let message_id = r.message_id.as_ref().map(|m| m.0.clone());
            let mut row = Statement::new(insert)
                .bind(self.services.new_id()?)
                .bind(channel)
                .bind(channel)
                .bind(to.external_id.clone())
                .bind(pushed)
                .bind(metering)
                .bind(message_id.clone())
                .bind(module.map(str::to_string))
                .bind(msg.kind())
                .bind(now)
                .bind(error.clone())
                .bind(key);
            if resumed {
                row = row
                    .bind(key)
                    .bind(channel)
                    .bind(channel)
                    .bind(to.external_id.clone())
                    .bind(msg.kind());
            }
            stmts.push(row);
            if resumed {
                stmts.push(
                    Statement::new(format!(
                        "UPDATE outbound_message SET push = ?, metering = 'metered', \
                         platform_message_id = coalesce(?, platform_message_id), error = ? \
                         WHERE id = ({PARTIAL_ROW})"
                    ))
                    .bind(pushed)
                    .bind(message_id)
                    .bind(error)
                    .bind(key)
                    .bind(channel)
                    .bind(channel)
                    .bind(to.external_id.clone())
                    .bind(msg.kind()),
                );
            }
        }
        Ok(stmts)
    }
}

/// The metered units the Message at `index` of a batch adds to
/// `usage_daily`: its [`SendReceipt::metered_units`], less its
/// [`SendReceipt::replayed_units`] when it is the first Message and only
/// part of it was replayed (#44). That is the resume of a Message the last
/// attempt stopped in: its first Push requests went out then and were
/// recorded with it (`unfinished`), so their 409s now must not count again.
/// A Message replayed whole is metered in full: an earlier attempt sent it
/// but may have died before recording anything, and the Kernel cannot tell.
///
/// The rule is an approximation with two blind spots (#62): it
/// over-counts when an earlier attempt's whole Message went out and was
/// recorded but its answer was lost (the retry replays it whole, 409 on
/// every request, and meters it again), and under-counts when the
/// `Db::batch` recording a partial send failed (the resume replays part of
/// the Message, discounts it, though nothing recorded those units). Exact
/// metering needs the batch key and Message index on each row.
fn units_to_meter(index: usize, r: &SendReceipt) -> u32 {
    debug_assert!(r.replayed_units <= r.metered_units, "{r:?}");
    if is_resumed_part(index, r) {
        r.metered_units - r.replayed_units
    } else {
        r.metered_units
    }
}

/// The first Message of the batch, part of it replayed (answered 409) and
/// part of it sent now: the resume of the Message a `PartialSend` stopped
/// in, already recorded with a "partly sent" row.
fn is_resumed_part(index: usize, r: &SendReceipt) -> bool {
    index == 0 && r.replayed_units > 0 && r.replayed_units < r.metered_units
}

/// The account's `channel_account.id` (binds: channel, external id).
const ACCOUNT: &str = "(SELECT id FROM channel_account WHERE channel = ? AND external_id = ?)";

/// The "partly sent" row a batch with this key wrote for a Message kind to
/// an account (binds: batch key, channel, channel, external id, kind).
const PARTIAL_ROW: &str = "SELECT id FROM outbound_message WHERE batch_key = ? AND channel = ? AND \
     channel_account_id = (SELECT id FROM channel_account WHERE channel = ? AND external_id = ?) \
     AND kind = ? AND error LIKE 'partly sent:%' ORDER BY sent_at DESC, id DESC LIMIT 1";
