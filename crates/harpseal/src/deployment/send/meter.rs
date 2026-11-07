//! Exact metering of a batch and its resumes (#62). Every Message sent gets
//! one `outbound_message` row carrying its batch key, its index in the
//! batch (`msg_index`) and the metered units recorded for it (`units`);
//! `usage_daily.metered` adds the same units. A later attempt with the same
//! key reads those rows first, so it knows exactly which Messages and
//! units are on record:
//!
//! - a unit the Channel sent now (not a replay) always counts: the
//!   platform charged it;
//! - a unit the platform answered as already accepted (LINE 409,
//!   [`SendReceipt::replayed_units`]) counts only beyond what the row of
//!   that Message records. It went out on an earlier attempt whose answer,
//!   or whose write, was lost; it is counted once, now.
//!
//! The row of a Message seen before is updated (units, id, "partly sent"
//! cleared once it is complete), never written twice.

use std::collections::HashMap;

use super::Deployment;
use crate::Result;
use crate::delivery::{BatchReceipt, ChannelAccountId, Delivery, SendReceipt};
use crate::deployment::day::civil_day;
use crate::message::Message;
use crate::port_types::Statement;

/// What an earlier attempt recorded for one Message of a batch key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Recorded {
    id: String,
    kind: String,
    units: u32,
    /// The row's `error` is NULL: the Message wholly went out.
    complete: bool,
}

/// The rows of a batch key, by Message index.
pub(super) type RecordedRows = HashMap<usize, Recorded>;

/// The send being recorded: `msgs` are the Messages this attempt covered,
/// `msgs[0]` being Message `first` of the batch.
pub(super) struct Rows<'a> {
    pub(super) to: &'a ChannelAccountId,
    pub(super) delivery: &'a Delivery,
    pub(super) msgs: &'a [Message],
    pub(super) first: usize,
    pub(super) module: Option<&'a str>,
    pub(super) key: &'a str,
    pub(super) recorded: &'a RecordedRows,
}

/// The metered units Message `r` adds to `usage_daily` when its row already
/// records `recorded` units: every unit sent now, plus the replayed ones no
/// row recorded yet.
pub(super) fn units_to_meter(r: &SendReceipt, recorded: u32) -> u32 {
    debug_assert!(r.replayed_units <= r.metered_units, "{r:?}");
    let replayed = r.replayed_units.min(r.metered_units);
    (r.metered_units - replayed) + replayed.saturating_sub(recorded)
}

impl Deployment {
    /// The rows earlier attempts with `key` wrote for `to`, by Message
    /// index (rows without an index, from before migration 0010, are
    /// left out).
    pub(super) async fn recorded(&self, to: &ChannelAccountId, key: &str) -> Result<RecordedRows> {
        let channel = to.channel.as_str();
        let rows = self
            .services
            .db
            .query(
                Statement::new(
                    "SELECT id, msg_index, kind, coalesce(units, 0) AS units, \
                     error IS NULL AS complete FROM outbound_message \
                     WHERE batch_key = ? AND msg_index IS NOT NULL AND channel = ? AND \
                     channel_account_id = (SELECT id FROM channel_account \
                     WHERE channel = ? AND external_id = ?) ORDER BY sent_at, id",
                )
                .bind(key)
                .bind(channel)
                .bind(channel)
                .bind(to.external_id.clone()),
            )
            .await?;
        let mut out = RecordedRows::new();
        for i in 0..rows.len() {
            let int = |c: &str| rows.get(i, c).and_then(|v| v.as_i64()).unwrap_or(0);
            let text = |c: &str| {
                rows.get(i, c)
                    .and_then(|v| v.as_text())
                    .unwrap_or_default()
                    .to_string()
            };
            let Ok(index) = usize::try_from(int("msg_index")) else {
                continue;
            };
            out.insert(
                index,
                Recorded {
                    id: text("id"),
                    kind: text("kind"),
                    units: u32::try_from(int("units")).unwrap_or(0),
                    complete: int("complete") != 0,
                },
            );
        }
        Ok(out)
    }

    /// Ensures the `channel_account` row, then per Message sent its usage
    /// upsert and its `outbound_message` row, inserted or (seen by an
    /// earlier attempt with the key) updated. The Message the batch
    /// stopped in, when some of its parts went out, has `error` set. One
    /// `Db::batch`: all commit or none do.
    pub(super) fn outbound_rows(
        &self,
        s: &Rows<'_>,
        batch: &BatchReceipt,
    ) -> Result<Vec<Statement>> {
        let now = self.services.now_ms();
        let channel = s.to.channel.as_str();
        let offset = self.config.utc_offset_minutes().unwrap_or(0);
        let day = civil_day(now, offset);
        let mut stmts = vec![
            Statement::new(
                "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?) ON CONFLICT (channel, external_id) DO NOTHING",
            )
            .bind(self.services.new_id()?)
            .bind(channel)
            .bind(s.to.external_id.clone())
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
        for (i, (msg, (r, error))) in s.msgs.iter().zip(rows).enumerate() {
            let index = s.first + i;
            // A row of another kind at this index is not this Message (the
            // key was reused for other Messages): it is left alone.
            let seen = s.recorded.get(&index).filter(|row| row.kind == msg.kind());
            let delta = units_to_meter(r, seen.map_or(0, |row| row.units));
            let units = seen.map_or(0, |row| row.units).saturating_add(delta);
            // A Message counts as free once, when it is first on record
            // complete and cost nothing.
            let newly_complete = error.is_none() && !seen.is_some_and(|row| row.complete);
            let free = newly_complete && units == 0;
            if delta > 0 || free {
                stmts.push(
                    Statement::new(
                        "INSERT INTO usage_daily (channel, day, metered, free) VALUES (?, ?, ?, ?) \
                         ON CONFLICT (channel, day) DO UPDATE SET \
                         metered = metered + excluded.metered, free = free + excluded.free",
                    )
                    .bind(channel)
                    .bind(day.clone())
                    .bind(i64::from(delta))
                    .bind(i64::from(free)),
                );
            }
            let pushed = r.pushed || matches!(s.delivery, Delivery::Push);
            let metering = if units > 0 { "metered" } else { "free" };
            let message_id = r.message_id.as_ref().map(|m| m.0.clone());
            let index = i64::try_from(index).unwrap_or(i64::MAX);
            stmts.push(match seen {
                // A complete row stays complete when a replay stops earlier.
                Some(row) => Statement::new(
                    "UPDATE outbound_message SET push = max(push, ?), metering = ?, units = ?, \
                     platform_message_id = coalesce(?, platform_message_id), \
                     error = CASE WHEN error IS NULL THEN NULL ELSE ? END WHERE id = ?",
                )
                .bind(pushed)
                .bind(metering)
                .bind(i64::from(units))
                .bind(message_id)
                .bind(error)
                .bind(row.id.clone()),
                None => Statement::new(
                    "INSERT INTO outbound_message (id, channel, channel_account_id, push, \
                     metering, platform_message_id, module, kind, sent_at, error, batch_key, \
                     msg_index, units) VALUES (?, ?, (SELECT id FROM channel_account \
                     WHERE channel = ? AND external_id = ?), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(self.services.new_id()?)
                .bind(channel)
                .bind(channel)
                .bind(s.to.external_id.clone())
                .bind(pushed)
                .bind(metering)
                .bind(message_id)
                .bind(s.module.map(str::to_string))
                .bind(msg.kind())
                .bind(now)
                .bind(error)
                .bind(s.key)
                .bind(index)
                .bind(i64::from(units)),
            });
        }
        Ok(stmts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delivery::Metering;

    fn receipt(units: u32, replayed: u32) -> SendReceipt {
        SendReceipt::new(None, Metering::Free, true)
            .with_metered_units(units)
            .with_replayed_units(replayed)
    }

    #[test]
    fn a_replay_counts_only_beyond_what_is_on_record() {
        // A fresh send: everything.
        assert_eq!(units_to_meter(&receipt(2, 0), 0), 2);
        // Replayed whole, recorded whole (the answer was lost): nothing.
        assert_eq!(units_to_meter(&receipt(2, 2), 2), 0);
        // Replayed in part, the partial send was recorded: the new unit.
        assert_eq!(units_to_meter(&receipt(2, 1), 1), 1);
        // Replayed in part, the write of the partial send failed: both.
        assert_eq!(units_to_meter(&receipt(2, 1), 0), 2);
        // Two replayed, one on record (the second answer was lost): 1 + 1.
        assert_eq!(units_to_meter(&receipt(3, 2), 1), 2);
        // Sent again on a platform without de-duplication: charged again.
        assert_eq!(units_to_meter(&receipt(1, 0), 1), 1);
    }
}
