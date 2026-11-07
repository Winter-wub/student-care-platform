//! The webhook path: verify and parse a call, claim each event id in
//! `inbound_event` before handling it (#35), mark the claim done after,
//! and log an `inbound.failed` Security Event for a failed event or for a
//! claim whose handling never finished (#44, ADR-0009).

use super::{Deployment, WebhookReport};
use crate::delivery::{ChannelAccountId, Inbound, WebhookRequest};
use crate::port_types::Statement;
use crate::{Error, Result, kernel};

/// Days a handled webhook event id is remembered (`inbound_event`).
pub(crate) const INBOUND_EVENT_DAYS: i64 = 7;
/// Most `inbound_event` rows one tick deletes.
pub(crate) const INBOUND_EVENT_PRUNE_LIMIT: i64 = 500;
/// How long a claim may stay unfinished before a redelivery of its event
/// takes it as abandoned (the process handling it died): 60 s, beyond a
/// platform's webhook timeout and a Worker's default CPU limit.
pub(crate) const INBOUND_CLAIM_LEASE_MS: i64 = 60_000;
/// The error logged for a claim whose handling never finished.
pub(crate) const ABANDONED: &str =
    "an earlier delivery's handling never finished (killed?); the redelivery is not answered";

/// What claiming an event id found.
enum Claim {
    /// The id is new: handle the event.
    New,
    /// Already claimed: skip it.
    Duplicate,
    /// Claimed over [`INBOUND_CLAIM_LEASE_MS`] ago and never finished;
    /// this call marked it done. Skip it, and log it as failed.
    Abandoned,
}

impl Deployment {
    /// Verifies and parses one webhook call for Channel `kind`, then
    /// handles each event on its own: a failing event is logged (Security
    /// Event `inbound.failed`) and does not stop the others. `Err` only when
    /// the call as a whole is refused: `BadSignature`, or the body could not
    /// be parsed.
    ///
    /// An event with a platform event id ([`Inbound::event_id`]) is first
    /// claimed in `inbound_event` with one conditional insert; an id this
    /// Channel already claimed is skipped before anything is handled or
    /// sent (counted in [`WebhookReport::duplicates`]), so a retried or
    /// redelivered event is answered once. The claim stays even when
    /// handling fails: the event was logged, and answering it twice is
    /// worse than once. Once handled (or logged as failed) the claim is
    /// marked done. A redelivery that finds a claim still not done after
    /// 60 seconds (the process handling it was killed)
    /// marks it done, logs `inbound.failed` once, and is still not
    /// answered (ADR-0009). Claims older than 7 days are deleted by
    /// [`Deployment::tick`], never on the webhook path.
    pub async fn handle_webhook(&self, kind: &str, req: WebhookRequest) -> Result<WebhookReport> {
        let channel = self.channel_or_err(kind)?;
        let channel_kind = channel.kind().as_str();
        let events = channel.parse_webhook(req).await?;
        let mut report = WebhookReport {
            events: events.len(),
            failed: Vec::new(),
            duplicates: 0,
        };
        for ev in events {
            let ev_kind = ev.kind();
            let from = ev.from().clone();
            let id = ev.event_id().map(str::to_string);
            if let Some(id) = &id {
                match self.claim_event(channel_kind, id).await {
                    Ok(Claim::New) => {}
                    Ok(Claim::Duplicate) => {
                        report.duplicates += 1;
                        continue;
                    }
                    Ok(Claim::Abandoned) => {
                        report.duplicates += 1;
                        let e = Error::Conflict(ABANDONED.into());
                        self.log_failed_event(&from, ev_kind, Some(id), &e).await;
                        report.failed.push((ev_kind, e));
                        continue;
                    }
                    Err(e) => {
                        self.log_failed_event(&from, ev_kind, Some(id), &e).await;
                        report.failed.push((ev_kind, e));
                        continue;
                    }
                }
            }
            if let Err(e) = self.handle_inbound(ev).await {
                self.log_failed_event(&from, ev_kind, id.as_deref(), &e)
                    .await;
                report.failed.push((ev_kind, e));
            }
            if let Some(id) = &id {
                self.finish_event(channel_kind, id).await;
            }
        }
        Ok(report)
    }

    /// Claims `(channel, event_id)`. One conditional insert, so two
    /// deliveries racing each other cannot both win; when the id is taken,
    /// one conditional update takes over a claim left unfinished past the
    /// lease, so only one redelivery logs it.
    async fn claim_event(&self, channel: &str, event_id: &str) -> Result<Claim> {
        let db = &self.services.db;
        let now = self.services.now_ms();
        let claimed = db
            .execute(
                Statement::new(
                    "INSERT INTO inbound_event (channel, event_id, received_at) VALUES (?, ?, ?) \
                     ON CONFLICT (channel, event_id) DO NOTHING",
                )
                .bind(channel)
                .bind(event_id)
                .bind(now),
            )
            .await?;
        if claimed.rows_affected == 1 {
            return Ok(Claim::New);
        }
        let taken = db
            .execute(
                Statement::new(
                    "UPDATE inbound_event SET done_at = ? WHERE channel = ? AND event_id = ? \
                     AND done_at IS NULL AND received_at < ?",
                )
                .bind(now)
                .bind(channel)
                .bind(event_id)
                .bind(now - INBOUND_CLAIM_LEASE_MS),
            )
            .await?;
        Ok(if taken.rows_affected == 1 {
            Claim::Abandoned
        } else {
            Claim::Duplicate
        })
    }

    /// Marks the claim done. Best effort: if it fails, a redelivery after
    /// the lease logs a spurious `inbound.failed` but still does not answer.
    async fn finish_event(&self, channel: &str, event_id: &str) {
        let stmt = Statement::new(
            "UPDATE inbound_event SET done_at = ? WHERE channel = ? AND event_id = ? \
             AND done_at IS NULL",
        )
        .bind(self.services.now_ms())
        .bind(channel)
        .bind(event_id);
        let _ = self.services.db.execute(stmt).await;
    }

    /// Deletes claims older than [`INBOUND_EVENT_DAYS`], at most
    /// [`INBOUND_EVENT_PRUNE_LIMIT`] per call (run by [`Deployment::tick`],
    /// so a backlog drains over several ticks). Returns the rows deleted.
    pub(crate) async fn prune_inbound_events(&self, now_ms: i64) -> Result<u64> {
        let cutoff = now_ms - INBOUND_EVENT_DAYS * 86_400_000;
        let stmt = Statement::new(
            "DELETE FROM inbound_event WHERE rowid IN (SELECT rowid FROM inbound_event \
             WHERE received_at < ? LIMIT ?)",
        )
        .bind(cutoff)
        .bind(INBOUND_EVENT_PRUNE_LIMIT);
        Ok(self.services.db.execute(stmt).await?.rows_affected)
    }

    /// Best effort: if even this write fails there is nowhere left to log.
    /// The account is linked when its `channel_account` row exists; the
    /// error text is cut to 200 chars. The details carry the platform's
    /// event id when there is one, never the event's payload.
    async fn log_failed_event(
        &self,
        from: &ChannelAccountId,
        event: &str,
        event_id: Option<&str>,
        e: &Error,
    ) {
        let Ok(id) = self.services.new_id() else {
            return;
        };
        let error: String = e.to_string().chars().take(200).collect();
        let stmt = Statement::new(
            "INSERT INTO security_event (id, at, kind, channel_account_id, details) VALUES \
             (?, ?, 'inbound.failed', \
             (SELECT id FROM channel_account WHERE channel = ? AND external_id = ?), \
             json_object('channel', ?, 'event', ?, 'event_id', ?, 'error', ?))",
        )
        .bind(id)
        .bind(self.services.now_ms())
        .bind(from.channel.as_str())
        .bind(from.external_id.clone())
        .bind(from.channel.as_str())
        .bind(event)
        .bind(event_id.map(str::to_string))
        .bind(error);
        let _ = self.services.db.execute(stmt).await;
    }

    /// Answers one event: the Kernel's chat flow (`kernel::flow`) handles
    /// Consent, Verification, Link, re-consent, unlink and fallback, and
    /// hands Actions of linked, consenting Students to their Module.
    async fn handle_inbound(&self, ev: Inbound) -> Result<()> {
        kernel::flow::handle(self, ev).await
    }
}
