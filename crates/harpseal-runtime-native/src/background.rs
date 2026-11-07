//! [`NativeBackground`]: the native [`harpseal::Spawner`] (#97).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use harpseal::{Background, BackgroundTask, Spawner};
use tokio::sync::{Notify, Semaphore};

/// Background tasks [`crate::serve`] lets run at once: webhook handling
/// for 16 chat accounts in parallel; more wait for a slot.
pub const BACKGROUND_LIMIT: usize = 16;

/// Runs webhook handling on tokio tasks, at most `limit` at once, and
/// knows when all of them have finished (for a graceful shutdown).
/// Cheap to clone; clones share the limit and the count.
#[derive(Clone, Debug)]
pub struct NativeBackground {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    slots: Arc<Semaphore>,
    /// Tasks spawned and not yet finished (waiting or running).
    pending: AtomicUsize,
    idle: Notify,
}

impl NativeBackground {
    /// At most `limit` tasks run at once (at least one).
    pub fn new(limit: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                slots: Arc::new(Semaphore::new(limit.max(1))),
                pending: AtomicUsize::new(0),
                idle: Notify::new(),
            }),
        }
    }

    /// The Core handle to put on requests:
    /// `router.layer(axum::Extension(bg.background()))`.
    pub fn background(&self) -> Background {
        Background::new(self.clone())
    }

    /// Tasks spawned and not yet finished.
    pub fn pending(&self) -> usize {
        self.inner.pending.load(Ordering::SeqCst)
    }

    /// Waits until every spawned task has finished, at most `timeout`.
    /// `true` when none is left. A task still running when the process
    /// exits leaves its event's claim unfinished; the lost-claim sweep
    /// logs it as `inbound.lost` (ADR-0009).
    pub async fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let idle = self.inner.idle.notified();
            tokio::pin!(idle);
            idle.as_mut().enable();
            if self.pending() == 0 {
                return true;
            }
            if tokio::time::timeout_at(deadline, idle).await.is_err() {
                return self.pending() == 0;
            }
        }
    }
}

impl Spawner for NativeBackground {
    fn spawn(&self, task: BackgroundTask) {
        // Called from a request handler, so a runtime is there; without
        // one (process shutting down) the task is dropped and its claim
        // left to the sweep.
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let inner = self.inner.clone();
        inner.pending.fetch_add(1, Ordering::SeqCst);
        let done = Finished(inner.clone());
        rt.spawn(async move {
            let _done = done;
            let _slot = inner.slots.clone().acquire_owned().await;
            task.await;
        });
    }
}

/// Counts a task as finished however it ends (done, panicked, dropped).
struct Finished(Arc<Inner>);

impl Drop for Finished {
    fn drop(&mut self) {
        if self.0.pending.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.0.idle.notify_waiters();
        }
    }
}
