//! The retire step of [`super`]: finish any retire still pending for the
//! Locale, retire the older revisions on every Channel, then re-lock the
//! accounts locked while the run went on.

use std::sync::Arc;

use super::super::{entries_from, revised};
use super::{Next, Step};
use crate::delivery::ChannelAccountId;
use crate::message::{Menu, MenuKey};
use crate::port_types::{Statement, Value};
use crate::{Channel, Error, MenuRetire, Result};

/// Accounts re-locked per Channel after a run.
const RELOCK_LIMIT: i64 = 500;

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
        for ch in channels {
            self.relock(ch.as_ref()).await?;
        }
        Ok(())
    }

    /// Locks again the Menu of `ch`'s accounts locked since the run
    /// started: an account unlinked (or revoked, or asked to consent
    /// again) mid-run may have been re-linked, or moved by the platform's
    /// batch, to the unlocked Menu. A failure is left alone: the account
    /// gets its Menu set again on its next Inbound.
    async fn relock(&self, ch: &dyn Channel) -> Result<()> {
        let rows = self
            .cx
            .services
            .db
            .query(
                Statement::new(
                    "SELECT external_id FROM channel_account WHERE channel = ? \
                     AND flow_state <> 'linked' AND updated_at >= ? ORDER BY id LIMIT ?",
                )
                .bind(ch.kind().as_str())
                .bind(self.started)
                .bind(RELOCK_LIMIT),
            )
            .await?;
        for i in 0..rows.len() {
            if let Some(id) = rows.get(i, "external_id").and_then(Value::as_text) {
                let account = ChannelAccountId::new(ch.kind(), id);
                let _ = ch.set_menu(&account, None).await;
            }
        }
        Ok(())
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
