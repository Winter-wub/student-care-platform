//! The Job [`super::RELINK`]: re-link one effective Locale's linked
//! accounts to its newest Menu revision, then retire the older revisions.
//!
//! Payload: `{"locale"}` from the save; its continuations add `revision`
//! (the revision they work for), `phase` (`relink` or `retire`), `channel`
//! (the Channel kind being worked on), `after` (the last
//! `channel_account.id` done), `started` (since when unlinks are looked
//! for), `hop` (for the continuation's key) and `waits`. Each run does one
//! chunk of [`crate::Channel::relink_chunk`] accounts per Channel, then
//! enqueues its continuation; a rate limit or a pending retire continues
//! after the time asked. An account the platform will never reach is
//! passed over (its Link marked unreachable); any other error fails the
//! run, and the Job's retry runs the same chunk again (a re-link is
//! idempotent).
//!
//! A run for revision `r` stops (`Ok`, nothing done) as soon as a newer
//! revision exists: that save enqueued its own Job, which re-links every
//! account and retires every older revision, `r` included. A retire still
//! pending on the platform is kept in `menu_retire` per Locale and
//! Channel, so the newer run takes it over and starts nothing new before
//! it is finished. Only one Job works on a revision: the first claims it
//! (`owner`); a later save's Job that finds the newest revision claimed,
//! or already `live`, does nothing.
//!
//! When everything is retired, accounts whose Menu was locked since the
//! run started (an unlink or a revoke while it ran) get the locked Menu
//! again, in case the re-link or the platform's batch moved them.

use std::sync::Arc;

use serde_json::{Value as Json, json};

use super::{RELINK, entries_from, entries_json, revised};
use crate::delivery::{ChannelAccountId, Timestamp};
use crate::kernel::text::Texts;
use crate::manifest::Job;
use crate::message::{Locale, Menu, MenuKey};
use crate::module::JobContext;
use crate::port_types::{Rows, Statement, Value};
use crate::{Channel, Error, MenuRetire, Result};

/// Waits one save's Job may chain (rate limits and retire polls); past it
/// the run fails with an error, which the Job status page shows.
pub(super) const MAX_WAITS: i64 = 200;
/// Accounts re-locked per Channel after a run.
const RELOCK_LIMIT: i64 = 500;

/// The newest revision of a Locale.
struct Newest {
    revision: i64,
    state: String,
}

pub(super) async fn run(cx: JobContext, job: Job) -> Result<()> {
    let p: Json = serde_json::from_str(&job.payload)
        .map_err(|e| Error::Invalid(format!("{RELINK} payload: {e}")))?;
    let locale = Locale::new(p["locale"].as_str().unwrap_or_default());
    let s = &cx.services;
    let Some(top) = newest(&cx, &locale).await? else {
        return Ok(());
    };
    let started = match p["revision"].as_i64() {
        // Superseded: the newer save's Job takes over (and any retire
        // still pending, from `menu_retire`).
        Some(r) if r != top.revision => return Ok(()),
        Some(_) => p["started"].as_i64().unwrap_or(0),
        None => {
            if top.state != "linking" || !claim(&cx, &locale, top.revision, &job.id).await? {
                return Ok(());
            }
            since(&cx, &locale).await?
        }
    };
    let d = cx.deployment.clone();
    let texts = Texts::load(s, d.catalog_arc(), locale.clone()).await?;
    let (_, salt) = super::revision(s, &locale).await?;
    let menu = d.menu_with(&locale, |k| texts.t(k));
    let menu = revised(
        menu,
        u32::try_from(top.revision).unwrap_or(0),
        salt.as_deref(),
    );
    let step = Step {
        cx: &cx,
        p: &p,
        locale: &locale,
        revision: top.revision,
        salt,
        started,
    };
    let channels = d.channels();
    let start = p["channel"]
        .as_str()
        .and_then(|k| channels.iter().position(|c| c.kind().as_str() == k))
        .unwrap_or(0);
    if p["phase"].as_str() != Some("retire") {
        let mut after = p["after"].as_str().unwrap_or_default().to_string();
        for ch in &channels[start..] {
            if let Some(next) = step.relink(ch.as_ref(), &menu, &after).await? {
                return step.continue_at(ch.as_ref(), "relink", next).await;
            }
            after.clear();
        }
        // Every linked account shows this revision: it is live.
        let live = Statement::new(
            "UPDATE menu_revision SET state = 'live', entries = ?, updated_at = ? \
             WHERE locale = ? AND revision = ? AND state = 'linking'",
        )
        .bind(entries_json(&menu))
        .bind(s.now_ms())
        .bind(locale.as_str())
        .bind(top.revision);
        s.db.execute(live).await?;
        return step.retire(channels, 0, &menu).await;
    }
    step.retire(channels, start, &menu).await
}

/// Where a continuation picks up: `after` the cursor, `secs` from now;
/// `wait` when it waits for the platform (counted against
/// [`MAX_WAITS`]), not for the next chunk.
struct Next {
    after: String,
    secs: i64,
    wait: bool,
}

struct Step<'a> {
    cx: &'a JobContext,
    p: &'a Json,
    locale: &'a Locale,
    revision: i64,
    salt: Option<String>,
    started: i64,
}

impl Step<'_> {
    /// One chunk of `ch`'s accounts after `after`: `None` when the
    /// Channel is done, else where to continue.
    async fn relink(&self, ch: &dyn Channel, menu: &Menu, after: &str) -> Result<Option<Next>> {
        let chunk = ch.relink_chunk().max(1);
        let rows = accounts(self.cx, ch, after, chunk).await?;
        let catalog = self.cx.deployment.catalog();
        let mut ids = Vec::new();
        let mut to = Vec::new();
        for i in 0..rows.len() {
            let text = |c: &str| {
                rows.get(i, c)
                    .and_then(Value::as_text)
                    .unwrap_or_default()
                    .to_string()
            };
            // Accounts of other Locales are passed over.
            if &catalog.effective(&Locale::new(text("locale"))) == self.locale {
                to.push(ChannelAccountId::new(ch.kind(), text("external_id")));
                ids.push(text("id"));
            }
        }
        let last = (0..rows.len())
            .last()
            .and_then(|i| rows.get(i, "id").and_then(Value::as_text))
            .map(str::to_string);
        let got = if to.is_empty() {
            crate::MenuRelinked::all(0)
        } else {
            ch.relink_menu(&to, menu).await
        };
        // Accounts the platform will never reach: their Link is
        // unreachable (as after a refused send), and the re-link goes on.
        let s = &self.cx.services;
        let mut marks = Vec::new();
        for (i, e) in &got.skipped {
            if let (Some(account), Error::Unreachable) = (to.get(*i), e.root()) {
                marks.extend(crate::kernel::flow::mark_unreachable(s, account));
            }
        }
        if !marks.is_empty() {
            s.db.batch(marks).await?;
        }
        match got.error {
            None if rows.len() < chunk => Ok(None),
            None => Ok(last.map(|after| Next {
                after,
                secs: 0,
                wait: false,
            })),
            Some(e) => match e.root() {
                Error::RateLimited { retry_after_secs } => {
                    let after = match got.done {
                        0 => after.to_string(),
                        n => ids[n.min(ids.len()) - 1].clone(),
                    };
                    Ok(Some(Next {
                        after,
                        secs: i64::from(*retry_after_secs).max(1),
                        wait: true,
                    }))
                }
                // The Job's retry runs this chunk again; nothing retired.
                _ => Err(e),
            },
        }
    }

    /// Retires the revisions older than this one on every Channel from
    /// `start` (first finishing any retire still pending there), then
    /// marks them retired and re-locks accounts unlinked meanwhile.
    async fn retire(&self, channels: &[Arc<dyn Channel>], start: usize, menu: &Menu) -> Result<()> {
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

    /// Enqueues the continuation at `ch`, `phase`, due `next.secs` from
    /// now; keyed per revision and hop so a re-run enqueues it once. Past
    /// [`MAX_WAITS`] waits the run fails instead (the Job's retries then
    /// fail too, and its `last_error` says why).
    async fn continue_at(&self, ch: &dyn Channel, phase: &str, next: Next) -> Result<()> {
        let hop = self.p["hop"].as_i64().unwrap_or(0) + 1;
        let waits = self.p["waits"].as_i64().unwrap_or(0) + i64::from(next.wait);
        if waits > MAX_WAITS {
            return Err(Error::Platform(format!(
                "{RELINK} {}: revision {} still not done after {MAX_WAITS} waits ({phase} on {})",
                self.locale.as_str(),
                self.revision,
                ch.kind().as_str()
            )));
        }
        let payload = json!({
            "locale": self.locale.as_str(),
            "revision": self.revision,
            "phase": phase,
            "channel": ch.kind().as_str(),
            "after": next.after,
            "started": self.started,
            "hop": hop,
            "waits": waits,
        });
        let key = format!("{RELINK}:{}:r{}:{hop}", self.locale.as_str(), self.revision);
        let at = Timestamp(self.cx.services.now_ms() + next.secs * 1000);
        self.cx
            .jobs
            .enqueue_unique(&key, RELINK, payload, Some(at))
            .await?;
        Ok(())
    }
}

/// The newest revision of `locale`, if any.
async fn newest(cx: &JobContext, locale: &Locale) -> Result<Option<Newest>> {
    let rows = cx
        .services
        .db
        .query(
            Statement::new(
                "SELECT revision, state FROM menu_revision WHERE locale = ? \
                 ORDER BY revision DESC LIMIT 1",
            )
            .bind(locale.as_str()),
        )
        .await?;
    Ok(rows
        .get(0, "revision")
        .and_then(Value::as_i64)
        .map(|revision| Newest {
            revision,
            state: rows
                .get(0, "state")
                .and_then(Value::as_text)
                .unwrap_or_default()
                .to_string(),
        }))
}

/// Since when a fresh run looks for accounts locked mid-run: the save of
/// the oldest revision not yet retired (a superseded run's accounts
/// included).
async fn since(cx: &JobContext, locale: &Locale) -> Result<i64> {
    let rows = cx
        .services
        .db
        .query(
            Statement::new(
                "SELECT MIN(created_at) AS t FROM menu_revision \
                 WHERE locale = ? AND revision > 0 AND state <> 'retired'",
            )
            .bind(locale.as_str()),
        )
        .await?;
    Ok(rows
        .get(0, "t")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| cx.services.now_ms()))
}

/// Claims the re-link of `revision` for Job `job` (again, on its retry).
async fn claim(cx: &JobContext, locale: &Locale, revision: i64, job: &str) -> Result<bool> {
    let r = cx
        .services
        .db
        .execute(
            Statement::new(
                "UPDATE menu_revision SET owner = ? WHERE locale = ? AND revision = ? \
                 AND state = 'linking' AND (owner IS NULL OR owner = ?)",
            )
            .bind(job)
            .bind(locale.as_str())
            .bind(revision)
            .bind(job),
        )
        .await?;
    Ok(r.rows_affected == 1)
}

/// Up to `chunk` accounts of `ch` with an active Link and an unlocked
/// Menu (`flow_state = 'linked'`), after `after`, by id.
async fn accounts(cx: &JobContext, ch: &dyn Channel, after: &str, chunk: usize) -> Result<Rows> {
    let st = Statement::new(
        "SELECT ca.id, ca.external_id, s.locale FROM link l \
         JOIN channel_account ca ON ca.id = l.channel_account_id \
         JOIN student s ON s.id = l.student_id \
         WHERE l.status = 'active' AND l.channel = ? AND ca.flow_state = 'linked' \
         AND ca.id > ? ORDER BY ca.id LIMIT ?",
    )
    .bind(ch.kind().as_str())
    .bind(after)
    .bind(i64::try_from(chunk).unwrap_or(i64::MAX));
    cx.services.db.query(st).await
}
