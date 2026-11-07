//! Test hooks into the Fake Channel: a callback run mid-send and a rate
//! limit on the next edit or delete.

use harpseal::{Error, Result};

use crate::FakeChannel;
use crate::record::{FAILURE_RETRY_AFTER_SECS, State};

/// A test's callback ([`FakeChannel::before_next_send`]).
pub(crate) struct Hook(pub(crate) Box<dyn FnOnce() + Send>);

impl core::fmt::Debug for Hook {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Hook")
    }
}

impl State {
    /// One edit or delete: `Err` when an armed failure fires now.
    pub(crate) fn maintenance(&mut self) -> Result<()> {
        match self.maint_fail_at {
            Some(0) => {
                self.maint_fails = self.maint_fails.saturating_sub(1);
                if self.maint_fails == 0 {
                    self.maint_fail_at = None;
                }
                Err(Error::RateLimited {
                    retry_after_secs: FAILURE_RETRY_AFTER_SECS,
                })
            }
            Some(n) => {
                self.maint_fail_at = Some(n - 1);
                Ok(())
            }
            None => Ok(()),
        }
    }
}

impl FakeChannel {
    /// Runs the [`FakeChannel::before_next_send`] callback, if armed
    /// (outside the lock: it may call back into this Channel).
    pub(crate) fn run_before_send(&self) {
        let hook = self.lock().before_send.take();
        if let Some(Hook(f)) = hook {
            f();
        }
    }

    /// Runs `f` once, right before the next batch or multicast is
    /// recorded: a test's way to change the world mid-send (e.g. retract
    /// while a Job is sending).
    pub fn before_next_send(&self, f: impl FnOnce() + Send + 'static) {
        self.lock().before_send = Some(Hook(Box::new(f)));
    }

    /// Makes the edit or delete after the next `ok` ones fail with
    /// `Error::RateLimited` (retry after [`FAILURE_RETRY_AFTER_SECS`]),
    /// once; the failed call changes nothing.
    pub fn fail_next_maintenance_at(&self, ok: usize) {
        self.fail_maintenance(ok, 1);
    }

    /// Like [`FakeChannel::fail_next_maintenance_at`], but the `times`
    /// edits or deletes after the next `ok` all fail.
    pub fn fail_maintenance(&self, ok: usize, times: usize) {
        let mut s = self.lock();
        s.maint_fail_at = (times > 0).then_some(ok);
        s.maint_fails = times;
    }
}
