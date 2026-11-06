//! The Kernel send path: one batch per reply token, Capability checks
//! before anything goes out, and one `outbound_message` row plus a
//! `usage_daily` upsert per Message, written in a single `Db::batch`.

use super::Deployment;
use crate::delivery::{Capability, ChannelAccountId, Delivery, Metering, ReplyToken, SendReceipt};
use crate::message::{Locale, Message};
use crate::port_types::Statement;
use crate::{Error, Result};

/// The Capability a Message needs beyond the Channel minimum.
fn required(msg: &Message) -> Option<Capability> {
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
    /// `outbound_message` row and counts once in `usage_daily (channel,
    /// day)`, all in one `Db::batch`. If the Channel stops part-way, the
    /// Messages already sent are recorded and its error is returned; if the
    /// write fails, the database error is returned (the Messages did go out).
    pub async fn send_batch(
        &self,
        to: &ChannelAccountId,
        reply: Option<&ReplyToken>,
        msgs: &[Message],
        locale: &Locale,
    ) -> Result<Vec<SendReceipt>> {
        self.deliver(to, reply, msgs, locale, None).await
    }

    /// [`Deployment::send_batch`] on behalf of `module` (stored on the rows).
    pub(super) async fn deliver(
        &self,
        to: &ChannelAccountId,
        reply: Option<&ReplyToken>,
        msgs: &[Message],
        locale: &Locale,
        module: Option<&str>,
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
        let batch = channel.send_batch(to, &delivery, msgs, locale).await;
        if !batch.receipts.is_empty() {
            let stmts = self.outbound_rows(to, msgs, &batch.receipts, module)?;
            self.services.db.batch(stmts).await?;
        }
        batch.into_result()
    }

    /// Ensures the `channel_account` row, then per Message the usage upsert
    /// followed by its `outbound_message` row: if any insert fails, the
    /// whole batch (usage included) rolls back.
    fn outbound_rows(
        &self,
        to: &ChannelAccountId,
        msgs: &[Message],
        receipts: &[SendReceipt],
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
        for (msg, r) in msgs.iter().zip(receipts) {
            let metered = r.metering == Metering::Metered;
            stmts.push(
                Statement::new(
                    "INSERT INTO usage_daily (channel, day, metered, free) VALUES (?, ?, ?, ?) \
                     ON CONFLICT (channel, day) DO UPDATE SET \
                     metered = metered + excluded.metered, free = free + excluded.free",
                )
                .bind(channel)
                .bind(day.clone())
                .bind(i64::from(metered))
                .bind(i64::from(!metered)),
            );
            stmts.push(
                Statement::new(
                    "INSERT INTO outbound_message (id, channel, channel_account_id, push, \
                     metering, platform_message_id, module, kind, sent_at) VALUES (?, ?, \
                     (SELECT id FROM channel_account WHERE channel = ? AND external_id = ?), \
                     ?, ?, ?, ?, ?, ?)",
                )
                .bind(self.services.new_id()?)
                .bind(channel)
                .bind(channel)
                .bind(to.external_id.clone())
                .bind(r.pushed)
                .bind(if metered { "metered" } else { "free" })
                .bind(r.message_id.as_ref().map(|m| m.0.clone()))
                .bind(module.map(str::to_string))
                .bind(msg.kind())
                .bind(now),
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
