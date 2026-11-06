//! The Kernel send path: one batch per reply token, Capability checks
//! before anything goes out, and one `outbound_message` row plus a
//! `usage_daily` upsert (its metered units) per Message, written in a
//! single `Db::batch`. A batch the Channel stops part-way is recorded as
//! far as it went and reported as `Error::PartialSend`.

use super::Deployment;
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
    /// [`Deployment::send_batch_with_key`] with the same key. When nothing
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
        // of it) count in `usage_daily.failed` (#26).
        let failed = match batch.error {
            Some(_) => msgs.len().saturating_sub(batch.receipts.len()),
            None => 0,
        };
        if batch.anything_sent() {
            let mut stmts = self.outbound_rows(to, &delivery, msgs, &batch, module)?;
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
        for ((msg, r), error) in msgs.iter().zip(rows).map(|(m, (r, e))| ((m, r), e)) {
            let units = r.metered_units;
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
            stmts.push(
                Statement::new(
                    "INSERT INTO outbound_message (id, channel, channel_account_id, push, \
                     metering, platform_message_id, module, kind, sent_at, error) VALUES (?, ?, \
                     (SELECT id FROM channel_account WHERE channel = ? AND external_id = ?), \
                     ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(self.services.new_id()?)
                .bind(channel)
                .bind(channel)
                .bind(to.external_id.clone())
                .bind(r.pushed || matches!(delivery, Delivery::Push))
                .bind(if units > 0 { "metered" } else { "free" })
                .bind(r.message_id.as_ref().map(|m| m.0.clone()))
                .bind(module.map(str::to_string))
                .bind(msg.kind())
                .bind(now)
                .bind(error),
            );
        }
        Ok(stmts)
    }
}

/// `YYYY-MM-DD` of the instant `ms` (UTC milliseconds) in a zone
/// `offset_minutes` east of UTC (Howard Hinnant's civil-from-days).
pub(crate) fn civil_day(ms: i64, offset_minutes: i32) -> String {
    let local = ms + i64::from(offset_minutes) * 60_000;
    let z = local.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::civil_day;

    #[test]
    fn days_follow_the_offset() {
        // 2026-11-05T00:00:00Z
        let ms = 1_793_836_800_000;
        assert_eq!(civil_day(ms, 0), "2026-11-05");
        assert_eq!(civil_day(ms - 1, 0), "2026-11-04");
        // 2026-11-04T17:00Z is already the 5th in Bangkok.
        assert_eq!(civil_day(ms - 7 * 3_600_000, 420), "2026-11-05");
        assert_eq!(civil_day(ms - 7 * 3_600_000 - 1, 420), "2026-11-04");
        assert_eq!(civil_day(0, 0), "1970-01-01");
        assert_eq!(civil_day(951_782_400_000, 0), "2000-02-29");
    }
}
