//! The failure emails, a backup's and retention's apart: one of each kind
//! per failure day (in the Deployment's `utc_offset`) to every active
//! Administrator, the recipients of the
//! Quota emails (`kernel::usage`). The day is claimed first, with the audit
//! row `backup.notify` (actor System) inserted only while the `setting`
//! row of that kind (`kernel.backup.notified`, `kernel.backup.prune_notified`,
//! Locale `''`) does not hold that day, and
//! the setting rewritten only if the audit row exists: two failures at
//! once send once. Sends that fail are not retried (the page shows the
//! failure anyway); the claim is not undone, so at most one email a day.

use serde_json::json;

use super::FULL_PATH;
use crate::audit::{self, Actor, RequestCtx};
use crate::kernel::text::Texts;
use crate::kernel::usage::administrators;
use crate::port_types::{Email, Statement};
use crate::{Deployment, Result};

/// What failed: each kind has its own email and its own day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Failure {
    /// The backup itself.
    Backup,
    /// Retention after a successful backup.
    Prune,
}

impl Failure {
    /// `setting` key holding the last day this kind's email went out.
    fn key(self) -> &'static str {
        match self {
            Failure::Backup => "kernel.backup.notified",
            Failure::Prune => "kernel.backup.prune_notified",
        }
    }
    fn texts(self) -> (&'static str, &'static str) {
        match self {
            Failure::Backup => ("kernel.backup.email_subject", "kernel.backup.email_body"),
            Failure::Prune => (
                "kernel.backup.prune_email_subject",
                "kernel.backup.prune_email_body",
            ),
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Failure::Backup => "backup",
            Failure::Prune => "retention",
        }
    }
}

/// Emails the Administrators unless today's email of this kind went out
/// already. Never fails: a failure to notify must not hide the error.
pub(super) async fn failure(d: &Deployment, kind: Failure) {
    let _ = claim_and_send(d, kind).await;
}

/// `Some(emails sent)` when this call claimed the day.
async fn claim_and_send(d: &Deployment, kind: Failure) -> Result<Option<usize>> {
    let s = d.services();
    let offset = d.config().utc_offset_minutes().unwrap_or(0);
    let day = crate::deployment::day::civil_day(s.now_ms(), offset);
    let admins = administrators(d).await?;
    let cond = Statement::new("(SELECT value FROM setting WHERE key = ? AND locale = '') IS NOT ?")
        .bind(kind.key())
        .bind(day.as_str());
    let after = json!({ "day": day, "kind": kind.as_str(), "recipients": admins.len() });
    let (audit, id) = audit::entry_if(
        s,
        &Actor::System,
        "backup.notify",
        None,
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
    .bind(kind.key())
    .bind(day.as_str())
    .bind(s.now_ms())
    .bind(id.as_str());
    s.db.batch(vec![audit, record]).await?;
    let won =
        s.db.query(Statement::new("SELECT 1 AS won FROM audit_log WHERE id = ?").bind(id))
            .await?;
    if won.rows.is_empty() {
        return Ok(None);
    }
    let texts = Texts::load(s, d.catalog_arc(), d.config().default_locale.clone()).await?;
    let org = crate::kernel::branding::org_name(d).await;
    let args = [
        ("org", org.as_str()),
        ("day", day.as_str()),
        ("path", FULL_PATH),
    ];
    let mut sent = 0;
    for a in &admins {
        let t = texts.in_locale(&a.locale);
        let email = Email {
            to: a.email.clone(),
            subject: t.tf(kind.texts().0, &args),
            text: t.tf(kind.texts().1, &args),
            html: None,
        };
        if s.email.send(email).await.is_ok() {
            sent += 1;
        }
    }
    Ok(Some(sent))
}
