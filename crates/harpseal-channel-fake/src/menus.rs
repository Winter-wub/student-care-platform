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

    /// Most accounts per `relink_menu` call ([`harpseal::Channel::relink_chunk`],
    /// default 100); shared by clones, so a test can shrink it after boot.
    pub fn set_relink_chunk(&self, n: usize) {
        self.lock().relink_chunk = Some(n.max(1));
    }

    pub(crate) fn relink(&self, to: &[ChannelAccountId], menu: &Menu) -> MenuRelinked {
        let mut s = self.lock();
        let stop = s.relink_fail_at.take().filter(|n| *n < to.len());
        let done = stop.unwrap_or(to.len());
        let accounts: Vec<ChannelAccountId> = to[..done]
            .iter()
            .filter(|a| !s.blocked.contains(a))
            .cloned()
            .collect();
        for a in &accounts {
            s.menus.insert(a.clone(), Some(menu.clone()));
        }
        s.menu_events.push(MenuEvent::Relinked {
            accounts,
            revision: menu.revision(),
        });
        match stop {
            Some(n) => MenuRelinked::stopped(n, Error::Platform("fake: re-link failed".into())),
            None => MenuRelinked::all(done),
        }
    }

    pub(crate) fn retire(&self, menu: &Menu, previous: &[Menu]) -> MenuRetire {
        self.lock().menu_events.push(MenuEvent::Retired {
            revisions: previous.iter().map(Menu::revision).collect(),
            live: menu.revision(),
        });
        MenuRetire::Done
    }
}
