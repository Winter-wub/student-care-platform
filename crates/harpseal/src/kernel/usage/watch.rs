//! Job `kernel.usage.watch`: emails every active
//! Administrator (Staff whose Roles grant `kernel.staff.manage`) once per
//! calendar month per Channel per level: at `Warn`, then again at `Over`.
//! A Channel that jumps past 100 % between two runs gets the `Over` email
//! only.
//!
//! [`Deployment::tick`] enqueues it ([`enqueue`]) on its first tick of
//! each 10-minute window (UTC-aligned), only when a Quota is in force:
//! `[quotas]` in the TOML or, when that is empty, a LINE package chosen in
//! the back office (#115; the tick reads that `setting` row only then). A
//! Deployment without a Quota never runs it, so its ticks stay as they
//! were. It is not a cron `JobSpec::schedule` for that reason, and because
//! a missed cron minute is skipped while a window is caught up by any later
//! tick in it.
//!
//! The limit is read at each run ([`super::package`]) and kept in the
//! record (`cap`). A record made under another limit is stale: its month
//! and level announce nothing for the limit in force, so a new email is due
//! at the next Warn or Over, and its retries are dropped. That covers a
//! mid-month package switch (Over under Free, then Warn and Over of Basic
//! each email once), a run that read the old limit and claimed just after
//! the switch, and a `[quotas]` number changed on a redeploy. A record from
//! before #115 has no `cap` and counts as current.
//!
//! No migration: the record is a `setting` row (see [`super::record`]). A
//! notification is claimed with one batch: the audit row `usage.notify`
//! (actor system) is inserted only while the record still reads what this
//! run read, and the record is rewritten only if that audit row exists.
//! Two runs at once therefore send once. A new month never matches the
//! record, so the levels start over. Emails go out after the claim;
//! addresses that fail stay in `pending` and are retried on the next runs
//! (never failing the Job), at most [`TRIES`](super::record::TRIES) sends per address per claim
//! and only while the address belongs to an active Administrator; then it
//! is dropped and counted in the record's `dropped`. Once nothing is left
//! to retry the claim settles: one audit row `usage.notify.done` with
//! `{channel, month, level, sent, failed, dropped_addresses_count}`
//! (`failed`: addresses dropped), written with the record by the same
//! gated batch, so at most once.
//!
//! At most once, not exactly once. The trade-offs, on purpose:
//! - The claim comes before the sends: a crash or restart part-way through
//!   the sends loses the rest of them (the record says claimed, with
//!   nothing pending).
//! - With no active Administrator the level is still claimed: nobody is
//!   emailed for it later, even once an Administrator exists.
//! - An `Over` claim made while a `Warn` send is still going (or still has
//!   addresses pending) replaces the record: the `Warn` pending list is
//!   dropped, since the `Over` email supersedes it.

use serde_json::json;

use super::quota::{ChannelMonth, Level};
use super::record::{Pending, Record};
use super::{FULL_PATH, store};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::delivery::Timestamp;
use crate::kernel::students::view::channel_label;
use crate::kernel::text::Texts;
use crate::manifest::Job;
use crate::message::Locale;
use crate::port_types::{Email, Statement, Value};
use crate::{Deployment, JobContext, Result};

/// The Job kind.
pub(super) const KIND: &str = "kernel.usage.watch";
/// One watch per window of this length.
const EVERY_MS: i64 = 10 * 60_000;
/// `setting` key prefix of the notification records.
pub(super) const KEY_PREFIX: &str = "kernel.usage.notified.";

/// An active Administrator.
pub(crate) struct Admin {
    pub(crate) email: String,
    pub(crate) locale: Locale,
}

/// Enqueues the watch for the 10-minute window of `now`, once however
/// many ticks run in it (a `job.unique_key` per window). Returns the Jobs
/// enqueued (0 or 1).
pub(crate) async fn enqueue(d: &Deployment, now: Timestamp) -> Result<usize> {
    let window = now.ms().div_euclid(EVERY_MS);
    let key = format!("{KIND}:{window}");
    let at = json!({ "window_at": window * EVERY_MS });
    let id = d.enqueue_unique(&key, KIND, at, Some(now)).await?;
    Ok(usize::from(id.is_some()))
}

pub(super) async fn run(cx: JobContext, _job: Job) -> Result<()> {
    let d = cx.deployment.clone();
    let (month, months) = store::this_month(&d).await?;
    for m in &months {
        watch(&d, &month, m).await?;
    }
    Ok(())
}

async fn watch(d: &Deployment, month: &str, m: &ChannelMonth) -> Result<()> {
    let key = format!("{KEY_PREFIX}{}", m.kind);
    let old = read(d, &key).await?;
    let rec = old.as_deref().and_then(Record::parse);
    // A record made under another limit (the LINE package changed, or
    // `[quotas]` on a redeploy) says nothing about this one (#115).
    let stale = rec.as_ref().is_some_and(|r| r.stale(m.cap));
    let due = m.level >= Level::Warn
        && rec
            .as_ref()
            .is_none_or(|r| r.month != month || stale || r.level < m.level);
    if due {
        let admins = administrators(d).await?;
        let claimed = Record::claim(month, m.level, m.cap);
        if !claim(d, &key, old.as_deref(), &claimed, m, admins.len()).await? {
            return Ok(());
        }
        let to = admins.into_iter().map(|a| (a, 0)).collect();
        return deliver(d, &key, m, claimed, to).await;
    }
    // A retry of the addresses the last sends could not reach.
    let Some(rec) = rec.filter(|r| r.month == month && !stale && !r.pending.is_empty()) else {
        return Ok(());
    };
    let Some(old) = old else { return Ok(()) };
    let taken = Record {
        pending: Vec::new(),
        ..rec.clone()
    };
    if !swap(d, &key, &old, &taken.to_value()).await? {
        return Ok(());
    }
    let to = administrators(d)
        .await?
        .into_iter()
        .filter_map(|a| {
            let tries = rec.pending.iter().find(|p| p.email == a.email)?.tries;
            Some((a, tries))
        })
        .collect();
    deliver(d, &key, m, taken, to).await
}

/// Sends `rec`'s email to each Administrator of `to` (each tried that many
/// times before) and records what went out and what is left to retry.
async fn deliver(
    d: &Deployment,
    key: &str,
    m: &ChannelMonth,
    rec: Record,
    to: Vec<(Admin, u32)>,
) -> Result<()> {
    let admins: Vec<&Admin> = to.iter().map(|(a, _)| a).collect();
    let failed = send(d, m, &rec, &admins).await;
    let failed: Vec<Pending> = to
        .iter()
        .filter(|(a, _)| failed.contains(&a.email))
        .map(|(a, tries)| Pending {
            email: a.email.clone(),
            tries: *tries,
        })
        .collect();
    let next = rec.after_send(to.len() - failed.len(), &failed);
    if !next.pending.is_empty() {
        swap(d, key, &rec.to_value(), &next.to_value()).await?;
        return Ok(());
    }
    // Settled: nothing left to retry. One `usage.notify.done` row, written
    // with the record (at most once: only while it still reads `rec`).
    let after = json!({
        "channel": m.kind, "month": next.month, "level": next.level.as_str(),
        "sent": next.sent, "failed": next.dropped,
        "dropped_addresses_count": next.dropped,
    });
    let old = rec.to_value();
    gated(
        d,
        key,
        Some(&old),
        &next.to_value(),
        "usage.notify.done",
        m,
        after,
    )
    .await?;
    Ok(())
}

async fn read(d: &Deployment, key: &str) -> Result<Option<String>> {
    let rows = d
        .services()
        .db
        .query(Statement::new("SELECT value FROM setting WHERE key = ? AND locale = ''").bind(key))
        .await?;
    Ok(rows
        .get(0, "value")
        .and_then(Value::as_text)
        .map(String::from))
}

/// Every active Administrator (Staff whose Roles grant
/// `kernel.staff.manage`), by email. Also the backup failure email's
/// recipients (#78).
pub(crate) async fn administrators(d: &Deployment) -> Result<Vec<Admin>> {
    let rows = d
        .services()
        .db
        .query(Statement::new(
            "SELECT DISTINCT s.email, s.locale FROM staff s \
             JOIN staff_role sr ON sr.staff_id = s.id \
             JOIN role_permission rp ON rp.role_id = sr.role_id \
             WHERE rp.permission_id = 'kernel.staff.manage' AND s.status = 'active' \
             ORDER BY s.email",
        ))
        .await?;
    let text = |i: usize, c: &str| rows.get(i, c).and_then(Value::as_text).map(String::from);
    Ok((0..rows.len())
        .filter_map(|i| {
            Some(Admin {
                email: text(i, "email")?,
                locale: Locale::new(text(i, "locale")?),
            })
        })
        .collect())
}

/// The claim: audit row `usage.notify` while the record still reads
/// `old`, then the new record if that row exists. `true` when this run won.
async fn claim(
    d: &Deployment,
    key: &str,
    old: Option<&str>,
    new: &Record,
    m: &ChannelMonth,
    recipients: usize,
) -> Result<bool> {
    let after = json!({
        "channel": m.kind, "month": new.month, "level": new.level.as_str(),
        "used": m.used(), "cap": m.cap, "recipients": recipients,
    });
    gated(d, key, old, &new.to_value(), "usage.notify", m, after).await
}

/// Audit row `action` (actor system, target the Channel) only while the
/// record still reads `old`, and the record `new` only if that row exists,
/// in one batch. `true` when both were written.
async fn gated(
    d: &Deployment,
    key: &str,
    old: Option<&str>,
    new: &str,
    action: &str,
    m: &ChannelMonth,
    after: serde_json::Value,
) -> Result<bool> {
    let s = d.services();
    let cond = Statement::new("(SELECT value FROM setting WHERE key = ? AND locale = '') IS ?")
        .bind(key)
        .bind(old.map(String::from));
    let target = Target::new("channel", m.kind.clone());
    let (audit, id) = audit::entry_if(
        s,
        &Actor::System,
        action,
        Some(&target),
        None,
        Some(after),
        &RequestCtx::default(),
        cond,
    )?;
    let record = Statement::new(
        "INSERT INTO setting (key, locale, value, updated_at) SELECT ?, '', ?, ? \
         WHERE EXISTS (SELECT 1 FROM audit_log WHERE id = ?) \
         ON CONFLICT (key, locale) DO UPDATE SET value = excluded.value, \
         updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(new)
    .bind(s.now_ms())
    .bind(id.as_str());
    s.db.batch(vec![audit, record]).await?;
    let won =
        s.db.query(Statement::new("SELECT 1 AS won FROM audit_log WHERE id = ?").bind(id))
            .await?;
    Ok(!won.rows.is_empty())
}

/// Replaces the record if it still reads `old`.
async fn swap(d: &Deployment, key: &str, old: &str, new: &str) -> Result<bool> {
    let s = d.services();
    let done =
        s.db.execute(
            Statement::new(
                "UPDATE setting SET value = ?, updated_at = ? \
                 WHERE key = ? AND locale = '' AND value = ?",
            )
            .bind(new)
            .bind(s.now_ms())
            .bind(key)
            .bind(old),
        )
        .await?;
    Ok(done.rows_affected == 1)
}

/// Sends the email of `rec.level` to each of `to`; returns the addresses
/// that failed.
async fn send(d: &Deployment, m: &ChannelMonth, rec: &Record, to: &[&Admin]) -> Vec<String> {
    let s = d.services();
    let Ok(texts) = Texts::load(s, d.catalog_arc(), d.config().default_locale.clone()).await else {
        return to.iter().map(|a| a.email.clone()).collect();
    };
    let org = crate::kernel::branding::org_name(d).await;
    let (used, cap) = (
        crate::ui::number(m.used()),
        crate::ui::number(m.cap.unwrap_or(0)),
    );
    let percent = m.percent().unwrap_or(0).to_string();
    let channel = channel_label(&m.kind);
    let month = crate::ui::thai_month(&rec.month);
    let args = [
        ("org", org.as_str()),
        ("channel", channel.as_str()),
        ("used", used.as_str()),
        ("cap", cap.as_str()),
        ("percent", percent.as_str()),
        ("month", month.as_str()),
        ("path", FULL_PATH),
    ];
    let subject_key = match rec.level {
        Level::Over => "kernel.usage.email_subject_over",
        _ => "kernel.usage.email_subject_warn",
    };
    let mut failed = Vec::new();
    for a in to {
        let t = texts.in_locale(&a.locale);
        let email = Email {
            to: a.email.clone(),
            subject: t.tf(subject_key, &args),
            text: t.tf("kernel.usage.email_body", &args),
            html: None,
        };
        if s.email.send(email).await.is_err() {
            failed.push(a.email.clone());
        }
    }
    failed
}
