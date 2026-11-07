//! The retire step of [`super`]: finish any retire still pending for the
//! Locale, retire the older revisions on every Channel, then re-lock the
//! accounts locked while the run went on.

use std::sync::Arc;

use super::super::{entries_from, revised};
use super::{Next, Step};
use crate::delivery::ChannelAccountId;
use crate::message::{Menu, MenuKey};
use crate::port_types::{Rows, Statement, Value};
use crate::{Channel, Error, MenuRetire, Result};

/// Most accounts one run re-locks on a Channel: one `set_menu` each, so a
/// run stays under the 50 subrequests of a Workers free-plan invocation
/// even on a Channel whose re-link chunk is large (LINE: 2,500 per bulk
/// call).
pub(super) const RELOCK_PAGE: usize = 20;
/// Seconds before a re-lock page that hit a failure tries that account
/// again.
const RELOCK_RETRY_SECS: u32 = 60;

impl Step<'_> {
    /// Retires the revisions older than this one on every Channel from
    /// `start` (first finishing any retire still pending there), then
    /// marks them retired and re-locks accounts unlinked meanwhile.
    pub(super) async fn retire(
        &self,
        channels: &[Arc<dyn Channel>],
        start: usize,
        menu: &Menu,
    ) -> Result<()> {
        let previous = self.previous().await?;
        for ch in &channels[start..] {
            let pending = self.pending(ch.as_ref()).await?;
            if previous.is_empty() && pending.is_none() {
                continue;
            }
            let secs = match ch.retire_menus(menu, &previous, pending.as_deref()).await {
                Ok(MenuRetire::Done) => {
                    self.set_pending(ch.as_ref(), None).await?;
                    continue;
                }
                Ok(MenuRetire::Pending {
                    token,
                    retry_after_secs,
                }) => {
                    self.set_pending(ch.as_ref(), Some(&token)).await?;
                    retry_after_secs
                }
                Err(e) => match e.root() {
                    Error::RateLimited { retry_after_secs } => *retry_after_secs,
                    _ => return Err(e),
                },
            };
            let next = Next {
                after: String::new(),
                secs: i64::from(secs).max(1),
                wait: true,
            };
            return self.continue_at(ch.as_ref(), "retire", next).await;
        }
        let s = &self.cx.services;
        let retired = Statement::new(
            "UPDATE menu_revision SET state = 'retired', updated_at = ? \
             WHERE locale = ? AND revision < ? AND state <> 'retired'",
        )
        .bind(s.now_ms())
        .bind(self.locale.as_str())
        .bind(self.revision);
        s.db.execute(retired).await?;
        self.relock(channels, 0, "").await
    }

    /// Locks again the Menu of the accounts whose Link was revoked (an
    /// unlink or a Staff revoke) or that were asked to consent again since
    /// the run started, on every Channel from `start` after `after`: the
    /// re-link or the platform's batch may have moved them to the unlocked
    /// Menu. Onboarding accounts never had it and are left alone. Each
    /// account costs one `set_menu`, so a page holds at most
    /// [`RELOCK_PAGE`] (or the Channel's smaller [`Channel::relink_chunk`])
    /// per run, then a continuation. An account the platform will never
    /// reach is passed over; any other failure ends the page before that
    /// account, which the continuation tries again after a wait.
    pub(super) async fn relock(
        &self,
        channels: &[Arc<dyn Channel>],
        start: usize,
        after: &str,
    ) -> Result<()> {
        let mut after = after.to_string();
        for ch in &channels[start..] {
            let page = ch.relink_chunk().clamp(1, RELOCK_PAGE);
            let rows = self.locked_since(ch.as_ref(), &after, page).await?;
            for i in 0..rows.len() {
                let text = |c: &str| rows.get(i, c).and_then(Value::as_text).map(str::to_string);
                let (Some(id), Some(external)) = (text("id"), text("external_id")) else {
                    continue;
                };
                let account = ChannelAccountId::new(ch.kind(), external);
                match ch.set_menu(&account, None).await {
                    Ok(()) => {}
                    Err(e) if matches!(e.root(), Error::Unreachable | Error::Invalid(_)) => {}
                    Err(e) => {
                        // Not done: the next hop starts at this account.
                        let secs = match e.root() {
                            Error::RateLimited { retry_after_secs } => *retry_after_secs,
                            _ => RELOCK_RETRY_SECS,
                        };
                        let next = Next {
                            after,
                            secs: i64::from(secs).max(1),
                            wait: true,
                        };
                        return self.continue_at(ch.as_ref(), "relock", next).await;
                    }
                }
                after = id;
            }
            if rows.len() == page {
                let next = Next {
                    after,
                    secs: 0,
                    wait: false,
                };
                return self.continue_at(ch.as_ref(), "relock", next).await;
            }
            after.clear();
        }
        Ok(())
    }

    /// Up to `chunk` accounts of `ch` after `after` (by id) whose Menu is
    /// locked now and was locked by a revoke or a re-consent since the run
    /// started.
    async fn locked_since(&self, ch: &dyn Channel, after: &str, chunk: usize) -> Result<Rows> {
        let st = Statement::new(
            "SELECT ca.id, ca.external_id FROM channel_account ca \
             WHERE ca.channel = ? AND ca.id > ? AND ca.flow_state <> 'linked' \
             AND (ca.flow_state = 'awaiting_reconsent' AND ca.updated_at >= ? \
             OR EXISTS (SELECT 1 FROM link l WHERE l.channel_account_id = ca.id \
             AND l.status = 'revoked' AND l.revoked_at >= ?)) \
             ORDER BY ca.id LIMIT ?",
        )
        .bind(ch.kind().as_str())
        .bind(after)
        .bind(self.started)
        .bind(self.started)
        .bind(i64::try_from(chunk).unwrap_or(i64::MAX));
        self.cx.services.db.query(st).await
    }

    /// The token of the retire `ch` is still working on for this Locale.
    async fn pending(&self, ch: &dyn Channel) -> Result<Option<String>> {
        let rows = self
            .cx
            .services
            .db
            .query(
                Statement::new("SELECT token FROM menu_retire WHERE locale = ? AND channel = ?")
                    .bind(self.locale.as_str())
                    .bind(ch.kind().as_str()),
            )
            .await?;
        Ok(rows
            .get(0, "token")
            .and_then(Value::as_text)
            .map(str::to_string))
    }

    /// Stores (`Some`) or clears (`None`) the pending retire of `ch`.
    async fn set_pending(&self, ch: &dyn Channel, token: Option<&str>) -> Result<()> {
        let s = &self.cx.services;
        let st = match token {
            Some(t) => Statement::new(
                "INSERT INTO menu_retire (locale, channel, token, updated_at) VALUES (?, ?, ?, ?) \
                 ON CONFLICT (locale, channel) DO UPDATE SET token = excluded.token, \
                 updated_at = excluded.updated_at",
            )
            .bind(self.locale.as_str())
            .bind(ch.kind().as_str())
            .bind(t)
            .bind(s.now_ms()),
            None => Statement::new("DELETE FROM menu_retire WHERE locale = ? AND channel = ?")
                .bind(self.locale.as_str())
                .bind(ch.kind().as_str()),
        };
        s.db.execute(st).await?;
        Ok(())
    }

    /// The revisions older than this one not yet retired, as Menus (with
    /// the salt: revision 0 never had one, so adapters leave it alone).
    async fn previous(&self) -> Result<Vec<Menu>> {
        let Ok(key) = MenuKey::for_locale(self.locale) else {
            return Ok(Vec::new());
        };
        let rows = self
            .cx
            .services
            .db
            .query(
                Statement::new(
                    "SELECT revision, entries FROM menu_revision \
                     WHERE locale = ? AND revision < ? AND state <> 'retired' ORDER BY revision",
                )
                .bind(self.locale.as_str())
                .bind(self.revision),
            )
            .await?;
        Ok((0..rows.len())
            .map(|i| {
                let revision = rows.get(i, "revision").and_then(Value::as_i64);
                let entries = rows.get(i, "entries").and_then(Value::as_text);
                let menu = Menu::keyed(key.clone(), entries_from(entries.unwrap_or("[]")));
                let revision = revision.and_then(|r| u32::try_from(r).ok()).unwrap_or(0);
                revised(menu, revision, self.salt.as_deref())
            })
            .collect())
    }
}
