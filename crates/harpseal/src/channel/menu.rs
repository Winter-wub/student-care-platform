//! What a Channel reports back from the Kernel's Menu maintenance
//! ([`super::Channel::relink_menu`], [`super::Channel::retire_menus`]).
//! Kept apart from `channel.rs` so each file stays small.

#[cfg(doc)]
use super::Channel;
use crate::Error;

/// What [`Channel::relink_menu`] got through.
#[derive(Debug)]
#[non_exhaustive]
pub struct MenuRelinked {
    /// Accounts done, counted from the start of `to` (skipped ones
    /// included).
    pub done: usize,
    /// Why it stopped before the end, if it did.
    pub error: Option<Error>,
    /// Accounts among the first `done` that cannot show the Menu for good
    /// (index in `to`, and why): [`Error::Unreachable`] (the Kernel marks
    /// the Link unreachable) or an id the platform refuses
    /// ([`Error::Invalid`]). They do not stop the re-link.
    pub skipped: Vec<(usize, Error)>,
}

impl MenuRelinked {
    /// Every one of `done` accounts.
    pub fn all(done: usize) -> Self {
        Self {
            done,
            error: None,
            skipped: Vec::new(),
        }
    }
    /// Stopped by `error` after `done` accounts.
    pub fn stopped(done: usize, error: Error) -> Self {
        Self {
            done,
            error: Some(error),
            skipped: Vec::new(),
        }
    }
    /// Records account `index` as skipped for good, because of `error`.
    pub fn skip(&mut self, index: usize, error: Error) {
        self.skipped.push((index, error));
    }
}

/// What [`Channel::retire_menus`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MenuRetire {
    /// The previous revisions are gone (or never existed).
    Done,
    /// The platform is still moving accounts off them: call again after
    /// `retry_after_secs` with `token`.
    Pending {
        /// The adapter's own encoding of what it is waiting for.
        token: String,
        /// Seconds to wait before asking again.
        retry_after_secs: u32,
    },
}
