//! One set of Messages to many accounts on one Channel (Announcements):
//! LINE-style multicast when the Channel has it, one keyed batch per
//! account otherwise. Recorded like every Kernel send: one
//! `outbound_message` row per recipient per Message and the metered units
//! in `usage_daily`, in one `Db::batch` per call.

use super::Deployment;
use super::day::civil_day;
use crate::delivery::{BatchReceipt, ChannelAccountId, SendReceipt};
use crate::message::{Locale, Message};
use crate::port_types::Statement;
use crate::{Error, Result};

/// What [`Deployment::send_many_with_key`] did, recipient by recipient.
#[derive(Debug)]
#[non_exhaustive]
pub struct ManySent {
    /// One entry per recipient reached, in `to` order: the receipts of
    /// the Messages (`receipts[j]` is Message `j`), or why that recipient
    /// failed (only when sending one account at a time: a multicast
    /// reports no per-recipient errors).
    pub results: Vec<Result<Vec<SendReceipt>>>,
    /// Why sending stopped before every recipient was reached (a failed
    /// multicast group, or a rate limit): resume with
    /// `to[results.len()..]` and the same key.
    pub stopped: Option<Error>,
}

impl Deployment {
    /// Sends `msgs` to every account in `to` (all on Channel `channel`)
    /// as Pushes, with idempotency `key`.
    ///
    /// With the Multicast Capability, the Channel sends groups of
    /// [`crate::Multicast::max_recipients`] in one request each; a failed
    /// group stops the call ([`ManySent::stopped`]) and `results` covers
    /// the groups that went out (as the [`crate::Multicast::send_many`]
    /// resume rule says): retrying `to[results.len()..]` with the same key
    /// lets the platform drop a request that did go out. Without it, each
    /// account gets [`Deployment::send_batch_with_key`] with key
    /// `<key>/<external id>`; an account's own failure (blocked:
    /// `Error::Unreachable`, which marks its Link) is that recipient's
    /// result and the next one is tried, while a rate limit or a partial
    /// send stops the call there.
    ///
    /// Every recipient reached gets one `outbound_message` row per Message
    /// (`module` stored on it) and the metered units count in
    /// `usage_daily`, written in one `Db::batch` for a multicast. A failed
    /// multicast group adds `msgs.len()` per recipient in it to
    /// `usage_daily.failed` (one at a time, [`Deployment::send_batch`]
    /// counts each account's failures).
    /// `Err` only when nothing could be tried (unknown Channel, an
    /// account on another Channel, a Message needing a Capability the
    /// Channel lacks: `Error::Unsupported`) or the write failed after a
    /// multicast. A multicast does not look at `blocked_at`: callers leave
    /// out accounts that blocked the bot (the Announcements child Job marks
    /// them `skipped`).
    pub async fn send_many_with_key(
        &self,
        channel: &str,
        to: &[ChannelAccountId],
        msgs: &[Message],
        locale: &Locale,
        module: Option<&str>,
        key: &str,
    ) -> Result<ManySent> {
        let ch = self.channel_or_err(channel)?;
        if to.iter().any(|a| a.channel != ch.kind()) {
            return Err(Error::Invalid(
                "every recipient must be on the channel".into(),
            ));
        }
        if to.is_empty() || msgs.is_empty() {
            return Ok(ManySent {
                results: to.iter().map(|_| Ok(Vec::new())).collect(),
                stopped: None,
            });
        }
        let caps = ch.capabilities();
        if let Some(c) = msgs
            .iter()
            .filter_map(super::send::required)
            .find(|c| !caps.has(*c))
        {
            return Err(Error::Unsupported(c));
        }
        let Some(multi) = ch.multicast() else {
            return Ok(self.one_by_one(to, msgs, locale, module, key).await);
        };
        let batch = multi.send_many(to, msgs, locale, key).await;
        let reached = batch.receipts.len() / msgs.len();
        // A failed group: each of its recipients missed every Message
        // (`usage_daily.failed`, #26; a 429 is retried, not counted).
        let failed = match &batch.error {
            Some(e) if !matches!(e.root(), Error::RateLimited { .. }) => {
                let group = multi.max_recipients().max(1).min(to.len() - reached);
                group * msgs.len()
            }
            _ => 0,
        };
        if reached > 0 {
            let mut stmts = self.many_rows(&to[..reached], msgs, &batch, module)?;
            if failed > 0 {
                stmts.push(self.failed_usage(channel, failed));
            }
            self.services.db.batch(stmts).await?;
        } else if failed > 0 {
            // Best effort: the Channel's error is what the caller needs.
            let _ = self
                .services
                .db
                .execute(self.failed_usage(channel, failed))
                .await;
        }
        let mut chunks = batch.receipts.chunks(msgs.len());
        let results = (0..reached)
            .map(|_| Ok(chunks.next().map(<[_]>::to_vec).unwrap_or_default()))
            .collect();
        Ok(ManySent {
            results,
            stopped: batch.error,
        })
    }

    async fn one_by_one(
        &self,
        to: &[ChannelAccountId],
        msgs: &[Message],
        locale: &Locale,
        module: Option<&str>,
        key: &str,
    ) -> ManySent {
        let mut results = Vec::with_capacity(to.len());
        for account in to {
            let k = format!("{key}/{}", account.external_id);
            match self.deliver_with(account, msgs, locale, module, &k).await {
                Ok(r) => results.push(Ok(r)),
                Err(e) if stops(&e) => {
                    return ManySent {
                        results,
                        stopped: Some(e),
                    };
                }
                Err(e) => results.push(Err(e)),
            }
        }
        ManySent {
            results,
            stopped: None,
        }
    }

    /// Per recipient reached: its `channel_account` row (ensured) and one
    /// `outbound_message` row per Message; one `usage_daily` upsert with
    /// the whole call's units.
    fn many_rows(
        &self,
        to: &[ChannelAccountId],
        msgs: &[Message],
        batch: &BatchReceipt,
        module: Option<&str>,
    ) -> Result<Vec<Statement>> {
        let now = self.services.now_ms();
        let offset = self.config.utc_offset_minutes().unwrap_or(0);
        let mut stmts = Vec::with_capacity(to.len() * (msgs.len() + 1) + 1);
        let (mut metered, mut free) = (0i64, 0i64);
        for (account, receipts) in to.iter().zip(batch.receipts.chunks(msgs.len())) {
            let channel = account.channel.as_str();
            stmts.push(
                Statement::new(
                    "INSERT INTO channel_account (id, channel, external_id, first_seen_at, \
                     updated_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT (channel, external_id) \
                     DO NOTHING",
                )
                .bind(self.services.new_id()?)
                .bind(channel)
                .bind(account.external_id.clone())
                .bind(now)
                .bind(now),
            );
            for (msg, r) in msgs.iter().zip(receipts) {
                let units = i64::from(r.metered_units);
                metered += units;
                free += i64::from(units == 0);
                stmts.push(
                    Statement::new(
                        "INSERT INTO outbound_message (id, channel, channel_account_id, push, \
                         metering, platform_message_id, module, kind, sent_at, error) VALUES \
                         (?, ?, (SELECT id FROM channel_account WHERE channel = ? AND \
                         external_id = ?), 1, ?, ?, ?, ?, ?, NULL)",
                    )
                    .bind(self.services.new_id()?)
                    .bind(channel)
                    .bind(channel)
                    .bind(account.external_id.clone())
                    .bind(if units > 0 { "metered" } else { "free" })
                    .bind(r.message_id.as_ref().map(|m| m.0.clone()))
                    .bind(module.map(str::to_string))
                    .bind(msg.kind())
                    .bind(now),
                );
            }
        }
        if let Some(first) = to.first() {
            stmts.push(
                Statement::new(
                    "INSERT INTO usage_daily (channel, day, metered, free) VALUES (?, ?, ?, ?) \
                     ON CONFLICT (channel, day) DO UPDATE SET \
                     metered = metered + excluded.metered, free = free + excluded.free",
                )
                .bind(first.channel.as_str())
                .bind(civil_day(now, offset))
                .bind(metered)
                .bind(free),
            );
        }
        Ok(stmts)
    }
}

/// Errors that stop a one-by-one send: the platform wants us to slow
/// down, or a recipient got part of the Messages (resume there).
fn stops(e: &Error) -> bool {
    matches!(e, Error::PartialSend { .. }) || matches!(e.root(), Error::RateLimited { .. })
}
