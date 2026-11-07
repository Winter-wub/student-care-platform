//! The Job [`super::RELINK`]: re-link one effective Locale's linked
//! accounts to its newest Menu revision, then retire the older revisions.
//!
//! Payload: `{"locale"}` from the save; its continuations add `revision`
//! (the revision they work for), `phase` (`relink` or `retire`), `channel`
//! (the Channel kind being worked on), `after` (the last
//! `channel_account.id` done), `token` (a pending [`MenuRetire`]) and
//! `hop`. Each run does one chunk of [`crate::Channel::relink_chunk`]
//! accounts per Channel, then enqueues its continuation; a rate limit or
//! a pending retire continues after the time asked. Any other error fails
//! the run, and the Job's retry runs the same step again (a re-link is
//! idempotent).
//!
//! A run for revision `r` stops (`Ok`, nothing done) as soon as a newer
//! revision exists: that save enqueued its own Job, which re-links every
//! account and retires every older revision, `r` included. Only one Job
//! works on a revision: the first claims it (`owner`); a later save's Job
//! that finds the newest revision claimed, or already `live`, does
//! nothing.

use std::sync::Arc;

use serde_json::{Value as Json, json};

use super::{RELINK, entries_from, entries_json};
use crate::delivery::{ChannelAccountId, Timestamp};
use crate::kernel::text::Texts;
use crate::manifest::Job;
use crate::message::{Locale, Menu, MenuKey};
use crate::module::JobContext;
use crate::port_types::{Rows, Statement, Value};
use crate::{Channel, Error, MenuRetire, Result};

/// Continuations one save's Job may chain (chunks, rate-limit waits and
/// retire polls); past it the chain stops and a later save finishes.
const MAX_HOPS: i64 = 500;

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
    match p["revision"].as_i64() {
        // Superseded: the newer save's Job takes over.
        Some(r) if r != top.revision => return Ok(()),
        Some(_) => {}
        None => {
            if top.state != "linking" || !claim(&cx, &locale, top.revision, &job.id).await? {
                return Ok(());
            }
        }
    }
    let d = cx.deployment.clone();
    let texts = Texts::load(s, d.catalog_arc(), locale.clone()).await?;
    let menu = d
        .menu_with(&locale, |k| texts.t(k))
        .with_revision(u32::try_from(top.revision).unwrap_or(0));
    let step = Step {
        cx: &cx,
        p: &p,
        locale: &locale,
        revision: top.revision,
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
        return step.retire(channels, 0, None, &menu).await;
    }
    step.retire(channels, start, p["token"].as_str(), &menu)
        .await
}

/// Where a continuation picks up.
struct Next {
    after: String,
    token: Option<String>,
    secs: i64,
}

struct Step<'a> {
    cx: &'a JobContext,
    p: &'a Json,
    locale: &'a Locale,
    revision: i64,
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
        match got.error {
            None if rows.len() < chunk => Ok(None),
            None => Ok(last.map(|after| Next {
                after,
                token: None,
                secs: 0,
            })),
            Some(e) => match e.root() {
                Error::RateLimited { retry_after_secs } => {
                    let after = match got.done {
                        0 => after.to_string(),
                        n => ids[n - 1].clone(),
                    };
                    Ok(Some(Next {
                        after,
                        token: None,
                        secs: i64::from(*retry_after_secs).max(1),
                    }))
                }
                // The Job's retry runs this chunk again; nothing retired.
                _ => Err(e),
            },
        }
    }

    /// Retires the revisions older than this one on every Channel from
    /// `start`, then marks them retired.
    async fn retire(
        &self,
        channels: &[Arc<dyn Channel>],
        start: usize,
        mut token: Option<&str>,
        menu: &Menu,
    ) -> Result<()> {
        let previous = self.previous().await?;
        if !previous.is_empty() {
            for ch in &channels[start..] {
                let next = match ch.retire_menus(menu, &previous, token).await {
                    Ok(MenuRetire::Done) => None,
                    Ok(MenuRetire::Pending {
                        token,
                        retry_after_secs,
                    }) => Some((Some(token), retry_after_secs)),
                    Err(e) => match e.root() {
                        Error::RateLimited { retry_after_secs } => {
                            Some((token.map(str::to_string), *retry_after_secs))
                        }
                        _ => return Err(e),
                    },
                };
                if let Some((token, secs)) = next {
                    let next = Next {
                        after: String::new(),
                        token,
                        secs: i64::from(secs).max(1),
                    };
                    return self.continue_at(ch.as_ref(), "retire", next).await;
                }
                token = None;
            }
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
        Ok(())
    }

    /// The revisions older than this one not yet retired, as Menus.
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
                Menu::keyed(key.clone(), entries_from(entries.unwrap_or("[]")))
                    .with_revision(revision.and_then(|r| u32::try_from(r).ok()).unwrap_or(0))
            })
            .collect())
    }

    /// Enqueues the continuation at `ch`, `phase`, due `next.secs` from
    /// now; keyed per revision and hop so a re-run enqueues it once.
    async fn continue_at(&self, ch: &dyn Channel, phase: &str, next: Next) -> Result<()> {
        let hop = self.p["hop"].as_i64().unwrap_or(0) + 1;
        if hop > MAX_HOPS {
            return Ok(());
        }
        let payload = json!({
            "locale": self.locale.as_str(),
            "revision": self.revision,
            "phase": phase,
            "channel": ch.kind().as_str(),
            "after": next.after,
            "token": next.token,
            "hop": hop,
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
