//! Job `kernel.usage.watch`: emails every active
//! Administrator (Staff whose Roles grant `kernel.staff.manage`) once per
//! calendar month per Channel per level: at `Warn`, then again at `Over`.
//! A Channel that jumps past 100 % between two runs gets the `Over` email
//! only.
//!
//! [`Deployment::tick`] enqueues it ([`enqueue`]) on its first tick of
//! each 10-minute window (UTC-aligned), only when the TOML has
//! `[quotas]`: a Deployment without a Quota never runs it, so its ticks
//! stay as they were. It is not a cron `JobSpec::schedule` for that
//! reason, and because a missed cron minute is skipped while a window is
//! caught up by any later tick in it.
//!
//! No migration: the record is a `setting` row `kernel.usage.notified.<kind>`
//! (Locale `''`, never shown on the Texts page) holding
//! `{"month":"2026-11","level":"warn","pending":[]}`. A notification is
//! claimed with one batch: the audit row `usage.notify` (actor system) is
//! inserted only while the record still reads what this run read, and the
//! record is rewritten only if that audit row exists. Two runs at once
//! therefore send once. A new month never matches the record, so the levels
//! start over. Emails go out after the claim; addresses that fail are put
//! back in `pending` and retried on the next run (never failing the Job),
//! as long as they still belong to an active Administrator.

use serde_json::{Value as Json, json};

use super::quota::{ChannelMonth, Level};
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

/// What a `setting` record says.
struct Record {
    month: String,
    level: Level,
    pending: Vec<String>,
}

impl Record {
    fn parse(v: &str) -> Option<Self> {
        let j: Json = serde_json::from_str(v).ok()?;
        let level = match j.get("level")?.as_str()? {
            "warn" => Level::Warn,
            "over" => Level::Over,
            _ => Level::Ok,
        };
        Some(Self {
            month: j.get("month")?.as_str()?.to_string(),
            level,
            pending: j
                .get("pending")
                .and_then(Json::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|e| e.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    fn to_value(&self) -> String {
        json!({ "month": self.month, "level": self.level.as_str(), "pending": self.pending })
            .to_string()
    }
}

/// An active Administrator.
struct Admin {
    email: String,
    locale: Locale,
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
    let due = m.level >= Level::Warn
        && rec
            .as_ref()
            .is_none_or(|r| r.month != month || r.level < m.level);
    if due {
        let admins = administrators(d).await?;
        let claimed = Record {
            month: month.to_string(),
            level: m.level,
            pending: Vec::new(),
        };
        if !claim(d, &key, old.as_deref(), &claimed, m, admins.len()).await? {
            return Ok(());
        }
        let failed = send(d, m, &claimed, &admins).await;
        return put_back(d, &key, claimed, failed).await;
    }
    // A retry of the addresses the last send could not reach.
    let Some(rec) = rec.filter(|r| r.month == month && !r.pending.is_empty()) else {
        return Ok(());
    };
    let Some(old) = old else { return Ok(()) };
    let pending = rec.pending;
    let taken = Record {
        month: rec.month,
        level: rec.level,
        pending: Vec::new(),
    };
    if !swap(d, &key, &old, &taken.to_value()).await? {
        return Ok(());
    }
    let admins: Vec<Admin> = administrators(d)
        .await?
        .into_iter()
        .filter(|a| pending.contains(&a.email))
        .collect();
    let failed = send(d, m, &taken, &admins).await;
    put_back(d, &key, taken, failed).await
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

async fn administrators(d: &Deployment) -> Result<Vec<Admin>> {
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

/// The claim: audit row while the record still reads `old`, then the new
/// record if that row exists. `true` when this run won.
async fn claim(
    d: &Deployment,
    key: &str,
    old: Option<&str>,
    new: &Record,
    m: &ChannelMonth,
    recipients: usize,
) -> Result<bool> {
    let s = d.services();
    let cond = Statement::new("(SELECT value FROM setting WHERE key = ? AND locale = '') IS ?")
        .bind(key)
        .bind(old.map(String::from));
    let after = json!({
        "channel": m.kind, "month": new.month, "level": new.level.as_str(),
        "used": m.used(), "cap": m.cap, "recipients": recipients,
    });
    let target = Target::new("channel", m.kind.clone());
    let (audit, id) = audit::entry_if(
        s,
        &Actor::System,
        "usage.notify",
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
    .bind(new.to_value())
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

/// Records the addresses that failed for the next run.
async fn put_back(d: &Deployment, key: &str, sent: Record, failed: Vec<String>) -> Result<()> {
    if failed.is_empty() {
        return Ok(());
    }
    let old = sent.to_value();
    let new = Record {
        pending: failed,
        ..sent
    };
    swap(d, key, &old, &new.to_value()).await.map(|_| ())
}

/// Sends the email of `rec.level` to each of `to`; returns the addresses
/// that failed.
async fn send(d: &Deployment, m: &ChannelMonth, rec: &Record, to: &[Admin]) -> Vec<String> {
    let s = d.services();
    let Ok(texts) = Texts::load(s, d.catalog_arc(), d.config().default_locale.clone()).await else {
        return to.iter().map(|a| a.email.clone()).collect();
    };
    let c = d.config();
    let org = c.branding.name.clone().unwrap_or_else(|| c.name.clone());
    let (used, cap) = (m.used().to_string(), m.cap.unwrap_or(0).to_string());
    let percent = m.percent().unwrap_or(0).to_string();
    let channel = channel_label(&m.kind);
    let args = [
        ("org", org.as_str()),
        ("channel", channel.as_str()),
        ("used", used.as_str()),
        ("cap", cap.as_str()),
        ("percent", percent.as_str()),
        ("month", rec.month.as_str()),
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
