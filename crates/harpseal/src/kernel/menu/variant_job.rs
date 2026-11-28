//! The Job [`KIND`] (#182): re-link the Menus of the Students whose Menu
//! variant may have changed, in bulk, one variant at a time.
//!
//! Queued, only when the Deployment has `[[menu.variants]]`, by the last
//! chunk of a roster import (every Student it wrote, `since` the import
//! started), by a Staff edit of an attribute a variant's `when` names
//! (`since` the edit) and by a Menu label save (every account of a
//! variant, `since` 0, from group 1: the whole Menu's own re-link,
//! [`super::RELINK`], passes over variant accounts). One Job per catalog
//! Locale.
//!
//! Payload: `{"locale", "since", "group", "tag"}`; continuations add `channel`,
//! `after` (the last `channel_account.id` done) and `hop`. Groups go in
//! order, 0 (no variant: the whole Menu at its newest revision) then each
//! variant (its own Menu, never revised); each Channel is worked through
//! a chunk of [`crate::Channel::relink_chunk`] accounts per run, the
//! accounts of an active Link with an unlocked Menu whose Student was
//! updated at or after `since` and belongs to the group now. A re-link is
//! idempotent: a failed run is retried from the same cursor; an account
//! the platform will never reach is passed over (its Link marked
//! unreachable).
//!
//! A variant Menu's earlier versions (before a label save) are not
//! retired: they stay on the platform until the next Menu generation, as
//! the Menus of Deployments without revisions do.

use serde_json::{Value as Json, json};

use super::variant::group_sql;
use crate::delivery::{ChannelAccountId, Timestamp};
use crate::kernel::text::Texts;
use crate::manifest::Job;
use crate::message::{Locale, Menu};
use crate::module::JobContext;
use crate::port_types::{Rows, Statement, Value};
use crate::{Channel, Deployment, Error, Result};

/// The Job kind.
pub(crate) const KIND: &str = "kernel.menu.variants";

/// Rate-limit waits one Job may chain; past it the run fails.
const MAX_WAITS: i64 = 200;

/// One Job per catalog Locale, to run in the caller's batch only if
/// `condition` holds (`None`: always); `tag` makes the dedupe key (e.g.
/// the import or audit id). Nothing when there are no variants.
pub(crate) fn enqueue_if(
    d: &Deployment,
    tag: &str,
    since: i64,
    group: usize,
    condition: Option<&Statement>,
) -> Result<Vec<Statement>> {
    if d.config().menu.variants.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for l in d.catalog().locales() {
        let key = format!("{KIND}:{tag}:{}", l.as_str());
        let payload = json!({
            "locale": l.as_str(),
            "since": since,
            "group": group,
            "tag": tag,
        });
        let (_, st) = match condition {
            Some(c) => d
                .jobs()
                .insert_unique_if(&key, KIND, &payload, None, c.clone())?,
            None => d.jobs().insert_unique(&key, KIND, &payload, None)?,
        };
        out.push(st);
    }
    Ok(out)
}

pub(super) async fn run(cx: JobContext, job: Job) -> Result<()> {
    let p: Json = serde_json::from_str(&job.payload)
        .map_err(|e| Error::Invalid(format!("{KIND} payload: {e}")))?;
    let d = cx.deployment.clone();
    let locale = Locale::new(p["locale"].as_str().unwrap_or_default());
    let since = p["since"].as_i64().unwrap_or(0);
    let variants = &d.config().menu.variants;
    let texts = Texts::load(&cx.services, d.catalog_arc(), locale.clone()).await?;
    let channels = crate::kernel::branding::drawn_all(&d).await?;
    let mut group = usize::try_from(p["group"].as_i64().unwrap_or(0)).unwrap_or(0);
    let (mut start, same) =
        crate::kernel::branding::resume_at(&d, &channels, p["channel"].as_str());
    let mut after = p["after"]
        .as_str()
        .filter(|_| same)
        .unwrap_or_default()
        .to_string();
    let run = Run {
        cx: &cx,
        p: &p,
        locale: &locale,
        since,
    };
    while group <= variants.len() {
        let variant = group.checked_sub(1).and_then(|i| variants.get(i));
        let menu = super::unlocked(&d, &texts, &locale, variant).await?;
        for ch in &channels[start..] {
            if let Some(next) = run.chunk(ch.as_ref(), &menu, group, &after).await? {
                return run.continue_at(ch.as_ref(), group, next).await;
            }
            after.clear();
        }
        start = 0;
        group += 1;
    }
    Ok(())
}

/// Where a continuation picks up.
struct Next {
    after: String,
    secs: i64,
    wait: bool,
}

struct Run<'a> {
    cx: &'a JobContext,
    p: &'a Json,
    locale: &'a Locale,
    since: i64,
}

impl Run<'_> {
    /// One chunk of `ch`'s accounts of `group` after `after`: `None` when
    /// the group is done on `ch`, else where to continue.
    async fn chunk(
        &self,
        ch: &dyn Channel,
        menu: &Menu,
        group: usize,
        after: &str,
    ) -> Result<Option<Next>> {
        let chunk = ch.relink_chunk().max(1);
        let rows = self.accounts(ch, group, after, chunk).await?;
        let catalog = self.cx.deployment.catalog();
        let (mut ids, mut to) = (Vec::new(), Vec::new());
        for i in 0..rows.len() {
            let text = |c: &str| {
                rows.get(i, c)
                    .and_then(Value::as_text)
                    .unwrap_or_default()
                    .to_string()
            };
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
        let resume = |done: usize| match done {
            0 => after.to_string(),
            n => ids[n.min(ids.len()) - 1].clone(),
        };
        let next = |after: String, secs: i64, wait: bool| Some(Next { after, secs, wait });
        match got.error {
            None if got.done < to.len() => Ok(next(resume(got.done), 0, false)),
            None if rows.len() < chunk => Ok(None),
            None => Ok(last.and_then(|after| next(after, 0, false))),
            Some(e) => match e.root() {
                Error::RateLimited { retry_after_secs } => Ok(next(
                    resume(got.done),
                    i64::from(*retry_after_secs).max(1),
                    true,
                )),
                _ => Err(e),
            },
        }
    }

    /// Up to `chunk` accounts of `ch` in `group`, after `after` by id.
    async fn accounts(
        &self,
        ch: &dyn Channel,
        group: usize,
        after: &str,
        chunk: usize,
    ) -> Result<Rows> {
        let variants = &self.cx.deployment.config().menu.variants;
        let (cond, binds) = group_sql(variants, group, "s.attributes");
        let mut st = Statement::new(format!(
            "SELECT ca.id, ca.external_id, s.locale FROM link l \
             JOIN channel_account ca ON ca.id = l.channel_account_id \
             JOIN student s ON s.id = l.student_id \
             WHERE l.status = 'active' AND l.channel = ? AND ca.flow_state = 'linked' \
             AND ca.id > ? AND s.updated_at >= ? AND {cond} ORDER BY ca.id LIMIT ?"
        ))
        .bind(ch.kind().as_str())
        .bind(after)
        .bind(self.since);
        st.params.extend(binds);
        let st = st.bind(i64::try_from(chunk).unwrap_or(i64::MAX));
        self.cx.services.db.query(st).await
    }

    /// Enqueues the continuation at `ch` and `group`.
    async fn continue_at(&self, ch: &dyn Channel, group: usize, next: Next) -> Result<()> {
        let hop = self.p["hop"].as_i64().unwrap_or(0) + 1;
        let waits = self.p["waits"].as_i64().unwrap_or(0) + i64::from(next.wait);
        if waits > MAX_WAITS {
            return Err(Error::Platform(format!(
                "{KIND} {}: still not done after {MAX_WAITS} waits (group {group} on {})",
                self.locale.as_str(),
                ch.kind().as_str()
            )));
        }
        let mut payload = self.p.clone();
        payload["group"] = json!(group);
        payload["channel"] = json!(ch.kind().as_str());
        payload["after"] = json!(next.after);
        payload["hop"] = json!(hop);
        payload["waits"] = json!(waits);
        let tag = self.p["tag"].as_str().unwrap_or_default();
        let key = format!("{KIND}:{tag}:{}:{hop}", self.locale.as_str());
        let at = Timestamp(self.cx.services.now_ms() + next.secs * 1000);
        self.cx
            .jobs
            .enqueue_unique(&key, KIND, payload, Some(at))
            .await?;
        Ok(())
    }
}
