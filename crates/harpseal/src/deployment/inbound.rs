//! The webhook path: verify and parse a call, claim each event id in
//! `inbound_event` before handling it (#35), mark the claim done after,
//! log an `inbound.failed` Security Event for a failed event, and an
//! `inbound.lost` one for a claim whose handling never finished, found by
//! a redelivery or by the tick (#44, ADR-0009).

use super::{Deployment, WebhookReport};
use crate::delivery::{ChannelAccountId, Inbound, WebhookRequest};
use crate::port_types::Statement;
use crate::{Error, Result, kernel};

/// Days a handled webhook event id is remembered (`inbound_event`).
pub(crate) const INBOUND_EVENT_DAYS: i64 = 7;
/// Most `inbound_event` rows one tick deletes.
pub(crate) const INBOUND_EVENT_PRUNE_LIMIT: i64 = 500;
/// How long a claim may stay unfinished before it counts as abandoned
/// (the process handling it died): 60 s, beyond a platform's webhook
/// timeout and a Worker's default CPU limit (30 s, configurable up to
/// 5 min). A slow File copy is the expected reason a live handler
/// exceeds it.
pub(crate) const INBOUND_CLAIM_LEASE_MS: i64 = 60_000;
/// Most abandoned claims one tick logs.
pub(crate) const INBOUND_LOST_SWEEP_LIMIT: i64 = 100;
/// The error logged for a claim whose handling never finished.
pub(crate) const ABANDONED: &str =
    "the handling of this event never finished (process killed?); it was not answered";

/// An event whose id this call claimed (or that has no id), still to be
/// handled.
pub(super) struct Claimed {
    ev: Inbound,
    kind: &'static str,
    from: ChannelAccountId,
    id: Option<String>,
}

impl Claimed {
    /// The chat account the event came from, as one key: events with the
    /// same key are handled one at a time, in order (#97).
    pub(super) fn account_key(&self) -> String {
        format!("{}:{}", self.from.channel.as_str(), self.from.external_id)
    }
}

/// What claiming an event id found.
enum Claim {
    /// The id is new: handle the event.
    New,
    /// Already claimed: skip it.
    Duplicate,
    /// Claimed over [`INBOUND_CLAIM_LEASE_MS`] ago and never finished;
    /// this call marked it done. Skip it, and log it as lost.
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
    /// marked done. A claim still not done 60 seconds after it was taken
    /// (the process handling it was killed) is logged once as
    /// `inbound.lost` and never answered: by the next redelivery of its
    /// event (counted in [`WebhookReport::failed`] only), else by
    /// [`Deployment::tick`] (ADR-0009). Claims older than 7 days are
    /// deleted by the tick, never on the webhook path.
    ///
    /// Every event of the call is claimed before the first is handled, then
    /// they are handled in call order, inside this call. The Router hands
    /// the handling to a [`crate::Background`] instead when the runtime
    /// provides one ([`Deployment::accept_webhook`]).
    pub async fn handle_webhook(&self, kind: &str, req: WebhookRequest) -> Result<WebhookReport> {
        let (mut report, claimed) = self.claim_webhook(kind, req).await?;
        for c in claimed {
            if let Some(failed) = self.handle_claimed(c).await {
                report.failed.push(failed);
            }
        }
        Ok(report)
    }

    /// The part of a webhook call that runs before the 200: verify and
    /// parse the call, then claim every event id in order. Returns the
    /// report so far (duplicates, lost redeliveries, failed claims, each
    /// already logged) and the events still to handle, in call order.
    pub(super) async fn claim_webhook(
        &self,
        kind: &str,
        req: WebhookRequest,
    ) -> Result<(WebhookReport, Vec<Claimed>)> {
        let channel = self.channel_or_err(kind)?;
        let events = channel.parse_webhook(req).await?;
        let mut report = WebhookReport {
            events: events.len(),
            failed: Vec::new(),
            duplicates: 0,
        };
        let mut claimed = Vec::with_capacity(events.len());
        for ev in events {
            let ev_kind = ev.kind();
            let from = ev.from().clone();
            let id = ev.event_id().map(str::to_string);
            if let Some(id) = &id {
                match self.claim_event(&from, ev_kind, id).await {
                    Ok(Claim::New) => {}
                    Ok(Claim::Duplicate) => {
                        report.duplicates += 1;
                        continue;
                    }
                    Ok(Claim::Abandoned) => {
                        let e = Error::Conflict(ABANDONED.into());
                        self.log_event(LOST, &from, ev_kind, Some(id), &e).await;
                        report.failed.push((ev_kind, e));
                        continue;
                    }
                    Err(e) => {
                        self.log_event(FAILED, &from, ev_kind, Some(id), &e).await;
                        report.failed.push((ev_kind, e));
                        continue;
                    }
                }
            }
            claimed.push(Claimed {
                ev,
                kind: ev_kind,
                from,
                id,
            });
        }
        Ok((report, claimed))
    }

    /// Handles one claimed event, logs `inbound.failed` when it fails, and
    /// marks its claim done either way. Returns the failure, if any.
    pub(super) async fn handle_claimed(&self, c: Claimed) -> Option<(&'static str, Error)> {
        let Claimed { ev, kind, from, id } = c;
        let failed = match self.handle_inbound(ev).await {
            Ok(()) => None,
            Err(e) => {
                self.log_event(FAILED, &from, kind, id.as_deref(), &e).await;
                Some((kind, e))
            }
        };
        if let Some(id) = &id {
            self.finish_event(from.channel.as_str(), id).await;
        }
        failed
    }

    /// Claims `(channel, event_id)` for an event of `kind` from `from`.
    /// One conditional insert, so two deliveries racing each other cannot
    /// both win; when the id is taken, one conditional update takes over a
    /// claim left unfinished past the lease, so only one caller logs it.
    async fn claim_event(&self, from: &ChannelAccountId, kind: &str, id: &str) -> Result<Claim> {
        let db = &self.services.db;
        let now = self.services.now_ms();
        let channel = from.channel.as_str();
        let claimed = db
            .execute(
                Statement::new(
                    "INSERT INTO inbound_event (channel, event_id, received_at, external_id, kind) \
                     VALUES (?, ?, ?, ?, ?) ON CONFLICT (channel, event_id) DO NOTHING",
                )
                .bind(channel)
                .bind(id)
                .bind(now)
                .bind(from.external_id.clone())
                .bind(kind),
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
                .bind(id)
                .bind(now - INBOUND_CLAIM_LEASE_MS),
            )
            .await?;
        Ok(if taken.rows_affected == 1 {
            Claim::Abandoned
        } else {
            Claim::Duplicate
        })
    }

    /// Marks the claim done. Best effort: if it fails (or the process dies
    /// between the reply and this write), the claim is later logged as a
    /// spurious `inbound.lost` though the event was answered.
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

    /// Run by [`Deployment::tick`]: claims left unfinished past the lease
    /// that no redelivery found are marked done and logged as
    /// `inbound.lost`, at most [`INBOUND_LOST_SWEEP_LIMIT`] per call. One
    /// `UPDATE … RETURNING`, so a claim is logged once even when a
    /// redelivery races the tick. Returns the claims logged.
    pub(crate) async fn sweep_lost_inbound(&self, now_ms: i64) -> Result<u64> {
        let rows = self
            .services
            .db
            .query(
                Statement::new(
                    "UPDATE inbound_event SET done_at = ? WHERE rowid IN (SELECT rowid FROM \
                     inbound_event WHERE done_at IS NULL AND received_at < ? LIMIT ?) \
                     RETURNING channel, event_id, external_id, kind",
                )
                .bind(now_ms)
                .bind(now_ms - INBOUND_CLAIM_LEASE_MS)
                .bind(INBOUND_LOST_SWEEP_LIMIT),
            )
            .await?;
        let text = |i: usize, c: &str| rows.get(i, c).and_then(|v| v.as_text()).map(str::to_string);
        let error = Error::Conflict(ABANDONED.into());
        let mut stmts = Vec::with_capacity(rows.len());
        for i in 0..rows.len() {
            let channel = text(i, "channel").unwrap_or_default();
            let account = text(i, "external_id");
            let kind = text(i, "kind").unwrap_or_else(|| "unknown".into());
            let id = text(i, "event_id");
            let row = self.event_row(
                LOST,
                &channel,
                account.as_deref(),
                &kind,
                id.as_deref(),
                &error,
            );
            stmts.push(row?);
        }
        let n = stmts.len() as u64;
        if !stmts.is_empty() {
            self.services.db.batch(stmts).await?;
        }
        Ok(n)
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
    async fn log_event(
        &self,
        kind: &'static str,
        from: &ChannelAccountId,
        event: &str,
        event_id: Option<&str>,
        e: &Error,
    ) {
        let channel = from.channel.as_str();
        let account = Some(from.external_id.as_str());
        if let Ok(stmt) = self.event_row(kind, channel, account, event, event_id, e) {
            let _ = self.services.db.execute(stmt).await;
        }
    }

    /// One Security Event of `kind` about an event: linked to the account
    /// when its `channel_account` row exists; details name the Channel,
    /// the sender's external id when known (claims made before migration
    /// 0009 have none), the event kind and id (never the payload)
    /// and the error cut to 200 chars.
    fn event_row(
        &self,
        kind: &'static str,
        channel: &str,
        external_id: Option<&str>,
        event: &str,
        event_id: Option<&str>,
        e: &Error,
    ) -> Result<Statement> {
        let error: String = e.to_string().chars().take(200).collect();
        Ok(Statement::new(
            "INSERT INTO security_event (id, at, kind, channel_account_id, details) VALUES \
             (?, ?, ?, (SELECT id FROM channel_account WHERE channel = ? AND external_id = ?), \
             json_object('channel', ?, 'external_id', ?, 'event', ?, 'event_id', ?, 'error', ?))",
        )
        .bind(self.services.new_id()?)
        .bind(self.services.now_ms())
        .bind(kind)
        .bind(channel.to_string())
        .bind(external_id.map(str::to_string))
        .bind(channel.to_string())
        .bind(external_id.map(str::to_string))
        .bind(event.to_string())
        .bind(event_id.map(str::to_string))
        .bind(error))
    }

    /// Answers one event: the Kernel's chat flow (`kernel::flow`) handles
    /// Consent, Verification, Link, re-consent, unlink and fallback, and
    /// hands Actions of linked, consenting Students to their Module.
    async fn handle_inbound(&self, ev: Inbound) -> Result<()> {
        kernel::flow::handle(self, ev).await
    }
}

/// Security Event kind of an event that failed while handled.
const FAILED: &str = "inbound.failed";
/// Security Event kind of a claim whose handling never finished.
const LOST: &str = "inbound.lost";
