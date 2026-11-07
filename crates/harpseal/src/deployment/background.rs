//! Webhook handling after the 200 (#97): the Router answers once the call
//! is verified and its events claimed, and the handling runs on a
//! [`Background`] the runtime provides, so a client (the platform, a
//! proxy) closing the connection no longer stops it half way.
//!
//! Ordering per chat account is kept here, not in the runtime: claimed
//! events wait in a FIFO per account, and one drain task per account
//! handles them one at a time. The runtime only spawns independent drain
//! tasks, so it may bound how many run at once without deadlock.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::inbound::{Claimed, INBOUND_CLAIM_LEASE_MS};
use super::{Deployment, WebhookReport};
use crate::Result;
use crate::delivery::WebhookRequest;

/// A unit of work handed to [`Spawner::spawn`].
pub type BackgroundTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Runs a task on its own, independent of the request that started it:
/// `tokio::spawn` on the native runtime, `ctx.wait_until` on Workers.
pub trait Spawner: Send + Sync {
    /// Starts `task` and returns at once. The task is never cancelled by
    /// the request ending; it may be lost if the process dies (the
    /// lost-claim sweep of ADR-0009 logs its events).
    fn spawn(&self, task: BackgroundTask);
}

/// The runtime's [`Spawner`], put on each request as an
/// `axum::Extension<Background>`. When a request carries one, the webhook
/// route answers 200 after the claims and handles the events through it
/// ([`Deployment::accept_webhook`]); without one it handles them inside
/// the request ([`Deployment::handle_webhook`]).
#[derive(Clone)]
pub struct Background(Arc<dyn Spawner>);

impl Background {
    /// Wraps a runtime's Spawner.
    pub fn new(spawner: impl Spawner + 'static) -> Self {
        Self(Arc::new(spawner))
    }
}

impl core::fmt::Debug for Background {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Background { .. }")
    }
}

impl Deployment {
    /// Verifies and parses one webhook call for Channel `kind` and claims
    /// its events, exactly as [`Deployment::handle_webhook`] does, then
    /// hands the handling to `bg` and returns without waiting for it. `Err`
    /// only when the call as a whole is refused (`BadSignature`, an
    /// unparseable body), so the caller still answers 401 or 400.
    ///
    /// The report covers the claim phase only (duplicates, lost
    /// redeliveries, failed claims); a handling failure is logged as
    /// `inbound.failed` later, as on the inline path. Events of one chat
    /// account are handled one at a time, in the order they were claimed,
    /// across calls as well as within one; events of different accounts
    /// may run concurrently. When an account's handling makes no progress
    /// for the claim lease (60 s: a hung handler, or a runtime that
    /// stopped the task without dropping it), the next call for that
    /// account takes its queue over instead of waiting behind it.
    pub async fn accept_webhook(
        self: &Arc<Self>,
        kind: &str,
        req: WebhookRequest,
        bg: &Background,
    ) -> Result<WebhookReport> {
        let (report, claimed) = self.claim_webhook(kind, req).await?;
        let mut batches: Vec<(String, Vec<Claimed>)> = Vec::new();
        for c in claimed {
            let key = c.account_key();
            match batches.iter_mut().find(|(k, _)| *k == key) {
                Some((_, batch)) => batch.push(c),
                None => batches.push((key, vec![c])),
            }
        }
        for (key, batch) in batches {
            self.enqueue_inbound(bg, key, batch);
        }
        Ok(report)
    }

    /// Appends `batch` to its account's queue; starts a drain task when
    /// none runs for that account, or when the running one made no
    /// progress for the lease (it takes the queue over).
    fn enqueue_inbound(self: &Arc<Self>, bg: &Background, key: String, batch: Vec<Claimed>) {
        let now = self.services.now_ms();
        let start = {
            let mut lanes = self.inbound_queues.lock();
            match lanes.get_mut(&key) {
                Some(lane) => {
                    lane.queue.push_back(batch);
                    if now - lane.progress_at > INBOUND_CLAIM_LEASE_MS {
                        lane.generation += 1;
                        lane.progress_at = now;
                        Some(lane.generation)
                    } else {
                        None
                    }
                }
                None => {
                    lanes.insert(
                        key.clone(),
                        Lane {
                            queue: VecDeque::from([batch]),
                            generation: 0,
                            progress_at: now,
                        },
                    );
                    Some(0)
                }
            }
        };
        if let Some(generation) = start {
            spawn_drain(self.clone(), bg.clone(), key, generation);
        }
    }
}

/// One account's claimed batches, and which drain task owns them.
struct Lane {
    queue: VecDeque<Vec<Claimed>>,
    /// The owning drain task; a takeover bumps it, and an older task that
    /// wakes up again stops without touching the lane.
    generation: u64,
    /// When the owner last took a batch (or was started).
    progress_at: i64,
}

/// Lanes per account key. A key is present exactly while a drain task
/// owns it (its queue may be empty: the last batch is being handled).
#[derive(Clone, Default)]
pub(super) struct AccountQueues(Arc<Mutex<HashMap<String, Lane>>>);

impl AccountQueues {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Lane>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn spawn_drain(d: Arc<Deployment>, bg: Background, key: String, generation: u64) {
    let spawner = bg.0.clone();
    spawner.spawn(Box::pin(drain(d, bg, key, generation)));
}

/// What a drain task does next.
enum Next {
    Handle(Vec<Claimed>),
    /// The queue is empty (key removed) or was taken over.
    Stop,
}

/// Handles `key`'s batches in order until its queue is empty, then
/// removes the key.
async fn drain(d: Arc<Deployment>, bg: Background, key: String, generation: u64) {
    let mut guard = DrainGuard {
        d: d.clone(),
        bg,
        key,
        generation,
        finished: false,
    };
    loop {
        let next = {
            let now = d.services.now_ms();
            let mut lanes = d.inbound_queues.lock();
            match lanes.get_mut(&guard.key) {
                Some(lane) if lane.generation == generation => match lane.queue.pop_front() {
                    Some(batch) => {
                        lane.progress_at = now;
                        Next::Handle(batch)
                    }
                    None => {
                        lanes.remove(&guard.key);
                        Next::Stop
                    }
                },
                _ => Next::Stop,
            }
        };
        match next {
            Next::Handle(batch) => {
                for c in batch {
                    // Failures are logged as `inbound.failed` by handle_claimed.
                    let _ = d.handle_claimed(c).await;
                }
            }
            Next::Stop => {
                guard.finished = true;
                return;
            }
        }
    }
}

/// Keeps an account from stalling when its drain task ends early (a
/// handler panicked, or the runtime dropped the task): the batches still
/// queued get a new drain task, or the key is removed. The event being
/// handled keeps its unfinished claim, which the lost-claim sweep logs.
struct DrainGuard {
    d: Arc<Deployment>,
    bg: Background,
    key: String,
    generation: u64,
    finished: bool,
}

impl Drop for DrainGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let restart = {
            let mut lanes = self.d.inbound_queues.lock();
            match lanes.get_mut(&self.key) {
                Some(lane) if lane.generation == self.generation => {
                    if lane.queue.is_empty() {
                        lanes.remove(&self.key);
                        None
                    } else {
                        lane.generation += 1;
                        lane.progress_at = self.d.services.now_ms();
                        Some(lane.generation)
                    }
                }
                _ => None,
            }
        };
        if let Some(generation) = restart {
            spawn_drain(
                self.d.clone(),
                self.bg.clone(),
                self.key.clone(),
                generation,
            );
        }
    }
}
