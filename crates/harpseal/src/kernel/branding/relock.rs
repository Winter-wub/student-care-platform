//! The Job [`RELOCK`] (#114): after a change of the locked Menu's image,
//! show the new locked Menu to every account that is not linked, then
//! retire the locked Menu's older revisions as ADR-0011 retires an
//! unlocked Menu's.
//!
//! Payload `{"revision"}` (the locked revision it works for) from the
//! save; continuations add `phase` (`relock` or `retire`), `channel`,
//! `after` (the last `channel_account.id` done), `hop` and `waits`. A run
//! re-locks a page of [`PAGE`] accounts per Channel (one `set_menu` each,
//! under the 50 subrequests of a Workers free-plan invocation), then
//! continues. An account the platform will never reach is passed over;
//! any other failure continues at that account after a wait. Once every
//! Channel is done, `retire_menus` gets the locked Menu at this revision
//! and every revision from 1 before it (revision 0, the unsalted name
//! another Deployment on the token may share, is never handed over); a
//! retire the platform is still working on is kept in `menu_retire` under
//! [`RETIRE_LOCALE`] and polled again. A newer change of the image makes
//! this run stop: the newer Job re-locks everyone and retires every older
//! revision.

use std::sync::Arc;

use serde_json::{Value as Json, json};

use super::store::{Saved, drawn_all};
use crate::delivery::{ChannelAccountId, Timestamp};
use crate::manifest::{Job, JobSpec};
use crate::message::{Menu, MenuKey};
use crate::module::{JobContext, ModuleRegistrar};
use crate::port_types::{Statement, Value};
use crate::{Channel, Error, MenuRetire, Result};

/// The Job kind.
pub(crate) const RELOCK: &str = "kernel.branding.relock";
/// The `menu_retire.locale` of the locked Menu's pending retire.
pub(crate) const RETIRE_LOCALE: &str = "~locked";
/// Accounts re-locked per Channel and run.
pub(crate) const PAGE: usize = 20;
/// Seconds before a failed account is tried again.
const RETRY_SECS: i64 = 60;
/// Waits one change's Job may chain; past it the run fails with an error.
const MAX_WAITS: i64 = 200;

pub(super) fn register(r: &mut ModuleRegistrar) {
    r.job(JobSpec::new(RELOCK), run);
}

/// The payload of the first run, for revision `revision`.
pub(crate) fn payload(revision: u32) -> Json {
    json!({ "revision": revision })
}

struct Run<'a> {
    cx: &'a JobContext,
    p: &'a Json,
    revision: u32,
    salt: String,
}

async fn run(cx: JobContext, job: Job) -> Result<()> {
    let p: Json = serde_json::from_str(&job.payload)
        .map_err(|e| Error::Invalid(format!("{RELOCK} payload: {e}")))?;
    let revision = p["revision"].as_u64().unwrap_or(0);
    let saved = Saved::load(&cx.services).await?;
    // Superseded by a newer change (its Job takes over), or nothing to do.
    let (Some(salt), true) = (
        saved.salt.clone(),
        u64::from(saved.locked_revision) == revision,
    ) else {
        return Ok(());
    };
    let d = cx.deployment.clone();
    let drawn = drawn_all(&d).await?;
    // Only the Channels with Menu images, drawn on the saved ones.
    let channels: Vec<Arc<dyn Channel>> = d
        .channels()
        .iter()
        .zip(drawn)
        .filter(|(c, _)| c.menu_art().is_some())
        .map(|(_, v)| v)
        .collect();
    let start = p["channel"]
        .as_str()
        .and_then(|k| channels.iter().position(|c| c.kind().as_str() == k))
        .unwrap_or(0);
    let r = Run {
        cx: &cx,
        p: &p,
        revision: saved.locked_revision,
        salt,
    };
    if p["phase"].as_str() == Some("retire") {
        return r.retire(&channels, start).await;
    }
    let after = p["after"].as_str().unwrap_or_default();
    r.relock(&channels, start, after).await
}

impl Run<'_> {
    async fn relock(&self, channels: &[Arc<dyn Channel>], start: usize, after: &str) -> Result<()> {
        let mut after = after.to_string();
        for ch in &channels[start..] {
            let rows = self.not_linked(ch.as_ref(), &after).await?;
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
                        let secs = match e.root() {
                            Error::RateLimited { retry_after_secs } => i64::from(*retry_after_secs),
                            _ => RETRY_SECS,
                        };
                        return self.continue_at(ch.as_ref(), "relock", &after, secs).await;
                    }
                }
                after = id;
            }
            if rows.len() == PAGE {
                return self.continue_at(ch.as_ref(), "relock", &after, 0).await;
            }
            after.clear();
        }
        self.retire(channels, 0).await
    }

    /// Up to [`PAGE`] accounts of `ch` after `after` without an unlocked
    /// Menu.
    async fn not_linked(&self, ch: &dyn Channel, after: &str) -> Result<crate::Rows> {
        let st = Statement::new(
            "SELECT id, external_id FROM channel_account \
             WHERE channel = ? AND id > ? AND flow_state <> 'linked' ORDER BY id LIMIT ?",
        )
        .bind(ch.kind().as_str())
        .bind(after)
        .bind(PAGE as i64);
        self.cx.services.db.query(st).await
    }

    /// The locked Menu at `revision`, salted; the adapter fills in its
    /// own entries.
    fn locked(&self, revision: u32) -> Menu {
        Menu::keyed(MenuKey::locked(), Vec::new())
            .with_revision(revision)
            .with_salt(self.salt.clone())
    }

    async fn retire(&self, channels: &[Arc<dyn Channel>], start: usize) -> Result<()> {
        let menu = self.locked(self.revision);
        let previous: Vec<Menu> = (1..self.revision).map(|r| self.locked(r)).collect();
        for ch in &channels[start..] {
            let pending = self.pending(ch.as_ref()).await?;
            if previous.is_empty() && pending.is_none() {
                continue;
            }
            let secs = match ch.retire_menus(&menu, &previous, pending.as_deref()).await {
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
            return self
                .continue_at(ch.as_ref(), "retire", "", i64::from(secs))
                .await;
        }
        Ok(())
    }

    async fn pending(&self, ch: &dyn Channel) -> Result<Option<String>> {
        let rows = self
            .cx
            .services
            .db
            .query(
                Statement::new("SELECT token FROM menu_retire WHERE locale = ? AND channel = ?")
                    .bind(RETIRE_LOCALE)
                    .bind(ch.kind().as_str()),
            )
            .await?;
        Ok(rows
            .get(0, "token")
            .and_then(Value::as_text)
            .map(str::to_string))
    }

    async fn set_pending(&self, ch: &dyn Channel, token: Option<&str>) -> Result<()> {
        let s = &self.cx.services;
        let st = match token {
            Some(t) => Statement::new(
                "INSERT INTO menu_retire (locale, channel, token, updated_at) VALUES (?, ?, ?, ?) \
                 ON CONFLICT (locale, channel) DO UPDATE SET token = excluded.token, \
                 updated_at = excluded.updated_at",
            )
            .bind(RETIRE_LOCALE)
            .bind(ch.kind().as_str())
            .bind(t)
            .bind(s.now_ms()),
            None => Statement::new("DELETE FROM menu_retire WHERE locale = ? AND channel = ?")
                .bind(RETIRE_LOCALE)
                .bind(ch.kind().as_str()),
        };
        s.db.execute(st).await?;
        Ok(())
    }

    /// Enqueues the continuation at `ch`, `phase`, `after`, due `secs`
    /// from now (a wait when `secs > 0`, counted against [`MAX_WAITS`]).
    async fn continue_at(
        &self,
        ch: &dyn Channel,
        phase: &str,
        after: &str,
        secs: i64,
    ) -> Result<()> {
        let hop = self.p["hop"].as_i64().unwrap_or(0) + 1;
        let waits = self.p["waits"].as_i64().unwrap_or(0) + i64::from(secs > 0);
        if waits > MAX_WAITS {
            return Err(Error::Platform(format!(
                "{RELOCK}: revision {} still not done after {MAX_WAITS} waits ({phase} on {})",
                self.revision,
                ch.kind().as_str()
            )));
        }
        let payload = json!({
            "revision": self.revision,
            "phase": phase,
            "channel": ch.kind().as_str(),
            "after": after,
            "hop": hop,
            "waits": waits,
        });
        let key = format!("{RELOCK}:r{}:{hop}", self.revision);
        let at = Timestamp(self.cx.services.now_ms() + secs.max(0) * 1000);
        self.cx
            .jobs
            .enqueue_unique(&key, RELOCK, payload, Some(at))
            .await?;
        Ok(())
    }
}
