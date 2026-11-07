//! Webhook handling after the 200 (#97): the Router answers once the call
//! is verified and its events claimed, and the handling runs on a
//! [`Background`] the runtime provides, so a client (the platform, a
//! proxy) closing the connection no longer stops it half way.
//!
//! Ordering per chat account is kept here, not in the runtime. Each call
//! starts one task per account it carries (under that request's own
//! `wait_until` on Workers); the task first waits for the account's
//! previous task to end, then handles its events one by one. A task
//! waiting its turn holds nothing: the runtime's concurrency slot
//! ([`Spawner::slot`]) is taken per event, so slots never wait on each
//! other and there is no deadlock.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};

use super::inbound::{Claimed, INBOUND_CLAIM_LEASE_MS};
use super::{Deployment, WebhookReport};
use crate::Result;
use crate::delivery::WebhookRequest;

/// A unit of work handed to [`Spawner::spawn`].
pub type BackgroundTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// A runtime concurrency slot, held while one event is handled; dropping
/// it frees the slot.
pub type Slot = Box<dyn Send>;

/// What [`Spawner::slot`] returns.
pub type SlotFuture = Pin<Box<dyn Future<Output = Slot> + Send + 'static>>;

/// Runs a task on its own, independent of the request that started it:
/// `tokio::spawn` on the native runtime, `ctx.wait_until` on Workers.
pub trait Spawner: Send + Sync {
    /// Starts `task` and returns at once. The task is never cancelled by
    /// the request ending; it may be lost if the process dies (the
    /// lost-claim sweep of ADR-0009 logs its events).
    fn spawn(&self, task: BackgroundTask);

    /// Waits for a free slot before one event is handled, so a runtime
    /// can bound how many events are handled at once. The default never
    /// waits (Workers: the platform bounds an isolate's work itself).
    fn slot(&self) -> SlotFuture {
        Box::pin(async { Box::new(()) as Slot })
    }
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
    /// may run concurrently. A claim's lease restarts when its handling
    /// starts; an event that waited in the queue past the lease was
    /// already logged `inbound.lost` by the tick and is skipped, never
    /// answered late.
    ///
    /// When an account's handling has not started a new event for the
    /// lease (60 s: a hung handler, or a task the runtime stopped without
    /// dropping it), its next call takes the account over instead of
    /// waiting; the tasks queued before stop at their next event, whose
    /// claims the tick logs as lost.
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
            self.start_turn(bg, key, batch);
        }
        Ok(report)
    }

    /// Starts one task for `batch`, queued behind the account's previous
    /// task (unless that one made no progress for the lease).
    fn start_turn(self: &Arc<Self>, bg: &Background, key: String, batch: Vec<Claimed>) {
        let now = self.services.now_ms();
        let turn = Turn::default();
        let (generation, seq, previous) = {
            let mut lanes = self.inbound_queues.lock();
            let lane = lanes.entry(key.clone()).or_insert_with(|| Lane {
                generation: 0,
                seq: 0,
                tail: None,
                progress_at: now,
            });
            let mut previous = lane.tail.take();
            if previous.is_some() && now - lane.progress_at > INBOUND_CLAIM_LEASE_MS {
                lane.generation += 1;
                previous = None;
            }
            if previous.is_none() {
                lane.progress_at = now;
            }
            lane.seq += 1;
            lane.tail = Some(turn.clone());
            (lane.generation, lane.seq, previous)
        };
        // Built before the task: a task dropped unpolled still ends its
        // turn and leaves the lane.
        let guard = TurnGuard {
            d: self.clone(),
            key,
            generation,
            seq,
            turn,
        };
        let task = run_turn(guard, previous, batch, bg.0.clone());
        bg.0.spawn(Box::pin(task));
    }
}

/// One account's chain of tasks.
struct Lane {
    /// Bumped by a takeover; tasks of an older generation stop.
    generation: u64,
    /// The newest task's number, so only it removes the lane.
    seq: u64,
    /// Ends when the newest task ends.
    tail: Option<Turn>,
    /// When an event of this account last started (or the lane was idle
    /// and got new work).
    progress_at: i64,
}

/// Lanes per account key; a key is present while a task for it exists.
#[derive(Clone, Default)]
pub(super) struct AccountQueues(Arc<Mutex<HashMap<String, Lane>>>);

impl AccountQueues {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Lane>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

async fn run_turn(
    guard: TurnGuard,
    previous: Option<Turn>,
    batch: Vec<Claimed>,
    spawner: Arc<dyn Spawner>,
) {
    if let Some(previous) = previous {
        previous.ended().await;
    }
    let d = guard.d.clone();
    for c in batch {
        let _slot = spawner.slot().await;
        let current = {
            let now = d.services.now_ms();
            let mut lanes = d.inbound_queues.lock();
            match lanes.get_mut(&guard.key) {
                Some(lane) if lane.generation == guard.generation => {
                    lane.progress_at = now;
                    true
                }
                _ => false,
            }
        };
        if !current {
            // Taken over: the rest stay claimed, for the tick to log.
            break;
        }
        // Failures are logged as `inbound.failed` by handle_claimed.
        let _ = d.handle_claimed(c).await;
    }
    drop(guard);
}

/// Ends a task's turn however the task ends (done, panicked, dropped
/// before or after its first poll), and removes the lane when this was
/// its newest task.
struct TurnGuard {
    d: Arc<Deployment>,
    key: String,
    generation: u64,
    seq: u64,
    turn: Turn,
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        self.turn.end();
        let mut lanes = self.d.inbound_queues.lock();
        if lanes
            .get(&self.key)
            .is_some_and(|l| l.generation == self.generation && l.seq == self.seq)
        {
            lanes.remove(&self.key);
        }
    }
}

/// A one-shot "this task ended" signal, without an async runtime.
#[derive(Clone, Default)]
struct Turn(Arc<Mutex<TurnState>>);

#[derive(Default)]
struct TurnState {
    ended: bool,
    waiters: Vec<Waker>,
}

impl Turn {
    fn lock(&self) -> MutexGuard<'_, TurnState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn end(&self) {
        let waiters = {
            let mut s = self.lock();
            s.ended = true;
            std::mem::take(&mut s.waiters)
        };
        waiters.into_iter().for_each(Waker::wake);
    }

    fn ended(&self) -> Ended {
        Ended(self.clone())
    }
}

/// Resolves once the [`Turn`] ended.
struct Ended(Turn);

impl Future for Ended {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut s = self.0.lock();
        if s.ended {
            return Poll::Ready(());
        }
        if !s.waiters.iter().any(|w| w.will_wake(cx.waker())) {
            s.waiters.push(cx.waker().clone());
        }
        Poll::Pending
    }
}
