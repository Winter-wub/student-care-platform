//! The Kernel send path: one batch per reply token, Capability checks
//! before anything goes out, and one `outbound_message` row plus a
//! `usage_daily` upsert (its metered units) per Message, written in a
//! single `Db::batch`. A batch the Channel stops part-way is recorded as
//! far as it went and reported as `Error::PartialSend`; a resume
//! ([`SendOptions::resume_after`]) meters exactly what the earlier
//! attempts did not record (`meter`, #62).
//!
//! `usage_daily.failed` counts failed attempts to the platform, not failed
//! Messages: a Message that fails twice and goes out on the third try
//! counts 2 there (and 1 in `metered`). `Error::RateLimited` adds nothing
//! (the caller retries later), and neither does a Push skipped before the
//! platform because the account blocked the bot.

mod meter;
mod resume;

pub use resume::SendOptions;

use super::Deployment;
use super::day::civil_day;
use crate::delivery::{Capability, ChannelAccountId, Delivery, ReplyToken, SendReceipt};
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
    /// `outbound_message` row (with its index in the batch and its metered
    /// units) and its metered units ([`SendReceipt::metered_units`]) count
    /// in `usage_daily (channel, day)`, all in one `Db::batch`. When the
    /// Channel returns an error, every Message that did not wholly go out
    /// (the one it stopped in included) adds 1 to `usage_daily.failed`
    /// (#26), in the same batch when something was sent, else on its own
    /// (best effort). A Push to an account that blocked the bot is skipped
    /// with `Error::Unreachable` before the Channel is called and counts
    /// nowhere; a send the Channel refuses with `Error::Unreachable` marks
    /// the account's Link `unreachable`.
    ///
    /// If the Channel stops part-way, what went out is recorded (the
    /// Message it stopped in gets a row with `error` set when some of its
    /// parts went out) and the result is [`Error::PartialSend`]: resume
    /// through [`Deployment::send_batch_with`] with
    /// `SendOptions::new().resume_after(&err)`, which restores the batch
    /// key this call chose ([`Error::batch_key`]). A keyed batch
    /// ([`Deployment::send_batch_with_key`]) may also be resent whole with
    /// the same key. Either way an earlier attempt's rows are found by key
    /// and index: a Push the platform answers as already accepted (LINE
    /// 409, [`SendReceipt::replayed_units`]) is metered only if no row
    /// recorded it, and a "partly sent" row is completed rather than a
    /// second one written. When nothing went out the Channel's error is
    /// returned as is. If the write fails, a database error is returned
    /// (the Messages did go out, unrecorded); it names the Channel's error
    /// too when there was one. It carries no key: only a keyed send can be
    /// retried exactly after it (the same key meters every Push once).
    pub async fn send_batch(
        &self,
        to: &ChannelAccountId,
        reply: Option<&ReplyToken>,
        msgs: &[Message],
        locale: &Locale,
    ) -> Result<Vec<SendReceipt>> {
        self.deliver_keyed(to, reply, msgs, locale, None, &SendOptions::new())
            .await
    }

    /// [`Deployment::send_batch`] with an idempotency `key` for the batch
    /// (e.g. derived from a Job id and the recipient). A retry that passes
    /// the same key, account and Messages lets the Channel de-duplicate
    /// requests that already went out (LINE `X-Line-Retry-Key`, honoured
    /// by LINE for 24 hours) and the Kernel meter each of them once.
    /// Without a key each call gets a fresh one. The same as
    /// [`Deployment::send_batch_with`] with `SendOptions::new().key(key)`.
    pub async fn send_batch_with_key(
        &self,
        to: &ChannelAccountId,
        reply: Option<&ReplyToken>,
        msgs: &[Message],
        locale: &Locale,
        key: &str,
    ) -> Result<Vec<SendReceipt>> {
        let opts = SendOptions::new().key(key);
        self.deliver_keyed(to, reply, msgs, locale, None, &opts)
            .await
    }

    /// [`Deployment::send_batch`] with [`SendOptions`]. `msgs` is always the
    /// whole batch; the send covers `msgs[opts.first_message()..]` and
    /// returns their receipts. After an [`Error::PartialSend`], pass
    /// `opts.resume_after(&err)` (it keeps `opts`' key, or takes the one
    /// the failed call used when `opts` had none): the Kernel sends from
    /// Message `sent`, hands the Channel its resume token (LINE rebuilds
    /// the failing request and those after it with the same retry keys, so
    /// no part reaches the Student twice) and meters exactly what no
    /// earlier attempt recorded. A further `PartialSend` counts `sent`
    /// over the whole batch again.
    pub async fn send_batch_with(
        &self,
        to: &ChannelAccountId,
        reply: Option<&ReplyToken>,
        msgs: &[Message],
        locale: &Locale,
        opts: &SendOptions,
    ) -> Result<Vec<SendReceipt>> {
        self.deliver_keyed(to, reply, msgs, locale, None, opts)
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
        self.deliver_keyed(to, reply, msgs, locale, module, &SendOptions::new())
            .await
    }

    /// A keyed Push on behalf of `module` ([`Deployment::send_many_with_key`]).
    pub(crate) async fn deliver_with(
        &self,
        to: &ChannelAccountId,
        msgs: &[Message],
        locale: &Locale,
        module: Option<&str>,
        key: &str,
    ) -> Result<Vec<SendReceipt>> {
        let opts = SendOptions::new().key(key);
        self.deliver_keyed(to, None, msgs, locale, module, &opts)
            .await
    }

    /// A keyed Push on behalf of `module` that starts at Message `first`
    /// of the batch, without a resume token: a notice (#73) resent after
    /// the Messages its rows record as wholly sent ([`Deployment::sent_prefix`]).
    pub(crate) async fn deliver_from(
        &self,
        to: &ChannelAccountId,
        msgs: &[Message],
        locale: &Locale,
        module: Option<&str>,
        key: &str,
        first: usize,
    ) -> Result<Vec<SendReceipt>> {
        let opts = SendOptions::new().key(key).from_message(first);
        self.deliver_keyed(to, None, msgs, locale, module, &opts)
            .await
    }

    async fn deliver_keyed(
        &self,
        to: &ChannelAccountId,
        reply: Option<&ReplyToken>,
        batch_msgs: &[Message],
        locale: &Locale,
        module: Option<&str>,
        opts: &SendOptions,
    ) -> Result<Vec<SendReceipt>> {
        let channel = self.channel_or_err(to.channel.as_str())?;
        let first = opts.first;
        let msgs = batch_msgs.get(first..).ok_or_else(|| {
            Error::Invalid(format!(
                "resume at Message {first} of a batch of {}",
                batch_msgs.len()
            ))
        })?;
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
        // Rows an earlier attempt with this key recorded (a fresh key has
        // none): read before anything goes out, so a failing read sends
        // nothing.
        let (key, recorded) = match &opts.key {
            Some(k) => (k.clone(), self.recorded(to, k).await?),
            None => (self.services.new_id()?, Default::default()),
        };
        let batch = channel
            .send_batch_resumed(to, &delivery, msgs, locale, &key, opts.resume.as_ref())
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
            let rows = meter::Rows {
                to,
                delivery: &delivery,
                msgs,
                first,
                module,
                key: &key,
                recorded: &recorded,
            };
            let mut stmts = self.outbound_rows(&rows, &batch)?;
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
        let nothing_before = first == 0 && batch.resume.is_none();
        if batch.receipts.is_empty() && batch.unfinished.is_none() && nothing_before {
            return Err(error);
        }
        Err(Error::PartialSend {
            sent: first + batch.receipts.len(),
            total: batch_msgs.len(),
            source: Box::new(error),
            resume: batch.resume,
            key: Some(key),
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
}
