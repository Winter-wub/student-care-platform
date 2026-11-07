//! The Menu re-link (#66): [`FakeChannel`] records each
//! `relink_menu` and `retire_menus` call as a [`MenuEvent`], applies a
//! re-link to the accounts' Menus, and can fail a re-link part-way.

use harpseal::{ChannelAccountId, Error, Menu, MenuRelinked, MenuRetire};

use crate::FakeChannel;

/// One re-link or retire the Kernel asked for, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MenuEvent {
    /// `relink_menu`: the accounts re-linked (those done, in order) to
    /// the Menu at `revision`.
    Relinked {
        /// The accounts now showing the Menu.
        accounts: Vec<ChannelAccountId>,
        /// Its Menu revision.
        revision: u32,
    },
    /// `retire_menus`: the revisions retired in favour of `live`.
    Retired {
        /// The previous revisions handed over.
        revisions: Vec<u32>,
        /// The revision every account was re-linked to.
        live: u32,
    },
}

impl FakeChannel {
    /// The re-links and retires so far.
    pub fn menu_events(&self) -> Vec<MenuEvent> {
        self.lock().menu_events.clone()
    }

    /// Makes the next `relink_menu` stop with `Error::Platform` after
    /// `ok` accounts (once), as a failed bulk call would.
    pub fn fail_next_relink_at(&self, ok: usize) {
        self.lock().relink_fail_at = Some(ok);
    }

    /// Makes the next `times` `relink_menu` calls stop at once with
    /// `Error::RateLimited` (retry after
    /// [`crate::FAILURE_RETRY_AFTER_SECS`]).
    pub fn rate_limit_relinks(&self, times: usize) {
        self.lock().relinks_limited = times;
    }

    /// Makes the next `times` `set_menu` calls fail with
    /// `Error::RateLimited` (retry after
    /// [`crate::FAILURE_RETRY_AFTER_SECS`]); the Menu is not changed.
    pub fn rate_limit_menus(&self, times: usize) {
        self.lock().menus_limited = times;
    }

    /// Most accounts per `relink_menu` call ([`harpseal::Channel::relink_chunk`],
    /// default 100); shared by clones, so a test can shrink it after boot.
    pub fn set_relink_chunk(&self, n: usize) {
        self.lock().relink_chunk = Some(n.max(1));
    }

    pub(crate) fn relink(&self, to: &[ChannelAccountId], menu: &Menu) -> MenuRelinked {
        let mut s = self.lock();
        if s.relinks_limited > 0 {
            s.relinks_limited -= 1;
            let e = Error::RateLimited {
                retry_after_secs: crate::FAILURE_RETRY_AFTER_SECS,
            };
            return MenuRelinked::stopped(0, e);
        }
        let stop = s.relink_fail_at.take().filter(|n| *n < to.len());
        let done = stop.unwrap_or(to.len());
        let mut got = match stop {
            Some(n) => MenuRelinked::stopped(n, Error::Platform("fake: re-link failed".into())),
            None => MenuRelinked::all(done),
        };
        let mut accounts = Vec::new();
        for (i, a) in to[..done].iter().enumerate() {
            // A blocked account is skipped, as `set_menu` would refuse it.
            if s.blocked.contains(a) {
                got.skip(i, Error::Unreachable);
            } else {
                s.menus.insert(a.clone(), Some(menu.clone()));
                accounts.push(a.clone());
            }
        }
        s.menu_events.push(MenuEvent::Relinked {
            accounts,
            revision: menu.revision(),
        });
        got
    }

    pub(crate) fn retire(&self, menu: &Menu, previous: &[Menu]) -> MenuRetire {
        self.lock().menu_events.push(MenuEvent::Retired {
            revisions: previous.iter().map(Menu::revision).collect(),
            live: menu.revision(),
        });
        MenuRetire::Done
    }
}
