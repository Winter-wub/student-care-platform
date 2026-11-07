//! One notify call (#73): load the Students and their live Links, then per
//! Student claim the (key, Student) row and chat or email.
//!
//! Idempotency, per Student (the key is `<module>:<key>`):
//! - a `done` row: reported again, nothing sent; a row another call holds:
//!   [`NotifyDefer::InProgress`] (the claim, `claim.rs`);
//! - chat: each Link's batch key is `notify:<module>:<key>:<student id>`.
//!   Messages its `outbound_message` rows record as wholly sent are not
//!   sent again on Channels without retry keys (`Deployment::sent_prefix`);
//!   on one with them (LINE) the whole batch goes again with the same key
//!   and the platform drops what it already has (ADR-0010);
//! - email: `via = 'email'` is written before the send, which carries the
//!   idempotency key `notify:<module>:<key>:<student id>`. A retaken row
//!   with an email in flight resends it when the sender de-duplicates
//!   (Resend), else counts it as sent (SMTP: at most once).

use std::collections::{HashMap, HashSet};

use super::claim::{self, Claim, Claimed};
use super::{
    Notice, Notified, NotifyDefer, NotifyFailure, NotifyOutcome, NotifyReport, NotifySkip,
    NotifyTo, Version,
};
use crate::delivery::{ChannelAccountId, ChannelKind, StudentId};
use crate::message::Locale;
use crate::port_types::{Email, Rows, Statement};
use crate::{Deployment, Error, Result};

/// A Student as loaded.
struct Student {
    id: String,
    code: String,
    email: Option<String>,
    locale: Locale,
    left: bool,
}

fn text(rows: &Rows, i: usize, col: &str) -> Option<String> {
    rows.get(i, col)
        .and_then(|v| v.as_text())
        .map(str::to_string)
}

fn marks(n: usize) -> String {
    vec!["?"; n].join(", ")
}

fn earlier(via: &str) -> NotifyOutcome {
    if via == "email" {
        NotifyOutcome::Emailed
    } else {
        NotifyOutcome::Sent
    }
}

/// One Student's send: the Notice, their version and what it may use.
struct Send<'a> {
    d: &'a Deployment,
    n: &'a Notice,
    s: &'a Student,
    locale: &'a Locale,
    v: &'a Version,
    links: Vec<ChannelAccountId>,
    /// `notify:<module>:<key>:<student id>`.
    key: String,
}

pub(super) async fn notify(d: &Deployment, n: &Notice, to: &[NotifyTo]) -> Result<NotifyReport> {
    n.check(to.len())?;
    let mut report = NotifyReport::default();
    if to.is_empty() {
        return Ok(report);
    }
    // Best effort: a failed prune leaves rows for the next call.
    let _ = claim::prune(d).await;
    let scoped = n.scoped();
    let students = load(d, to).await?;
    let ids: Vec<&str> = students.iter().map(|s| s.id.as_str()).collect();
    let links = links(d, &ids).await?;
    let done = claim::done(d, &scoped, &ids).await?;
    let default = d.config().default_locale.clone();
    let mut exhausted = HashSet::new();
    let (mut used, mut over) = (0usize, false);
    for r in to {
        let found = students.iter().find(|s| match r {
            NotifyTo::Id(id) => s.id == id.0,
            NotifyTo::Code(c) => &s.code == c,
        });
        let line = |student: Option<&Student>, outcome, again| Notified {
            to: r.clone(),
            student: student.map(|s| StudentId(s.id.clone())),
            outcome,
            again,
        };
        let Some(s) = found else {
            report.push(line(
                None,
                NotifyOutcome::Skipped(NotifySkip::Unknown),
                false,
            ));
            continue;
        };
        if let Some(via) = done.get(&s.id) {
            report.push(line(Some(s), earlier(via), true));
            continue;
        }
        let skip = |why| line(Some(s), NotifyOutcome::Skipped(why), false);
        if s.left {
            report.push(skip(NotifySkip::Left));
            continue;
        }
        let Some((locale, v)) = n.version(&s.locale, &default) else {
            report.push(skip(NotifySkip::NoText));
            continue;
        };
        let mine: Vec<ChannelAccountId> = links.get(&s.id).cloned().unwrap_or_default();
        let usable = mine.iter().filter(|a| !exhausted.contains(&a.channel));
        let cost = usable.count() * v.requests() + usize::from(s.email.is_some());
        over = over || (used > 0 && used + cost > n.budget);
        if over {
            let deferred = NotifyOutcome::Deferred(NotifyDefer::Budget);
            report.push(line(Some(s), deferred, false));
            continue;
        }
        let (claim, email_out) = match claim::claim(d, &scoped, &n.module, &s.id).await? {
            Claimed::Mine(c, email) => (c, email),
            Claimed::Done(via) => {
                report.push(line(Some(s), earlier(&via), true));
                continue;
            }
            Claimed::Busy => {
                let busy = NotifyOutcome::Deferred(NotifyDefer::InProgress);
                report.push(line(Some(s), busy, false));
                continue;
            }
        };
        used += cost;
        let one = Send {
            d,
            n,
            s,
            locale,
            v,
            links: mine,
            key: format!("notify:{scoped}:{}", s.id),
        };
        let outcome = match one.run(&claim, email_out, &mut exhausted).await {
            Ok(o) => o,
            Err(e) => {
                // What went out is recorded; free the row for the retry
                // (if this write fails too, the lease frees it).
                let _ = claim.release(d).await;
                return Err(e);
            }
        };
        report.push(line(Some(s), outcome, false));
    }
    let mut exhausted: Vec<ChannelKind> = exhausted.into_iter().collect();
    exhausted.sort();
    report.exhausted = exhausted;
    Ok(report)
}

impl Send<'_> {
    /// Chat on every usable Link, else email.
    async fn run(
        &self,
        claim: &Claim,
        email_out: bool,
        exhausted: &mut HashSet<ChannelKind>,
    ) -> Result<NotifyOutcome> {
        let d = self.d;
        if email_out {
            return self.email(claim, true, false).await;
        }
        let (mut failed, mut quota) = (false, false);
        if !self.links.is_empty() {
            if !claim.start(d, "chat").await? {
                return Ok(NotifyOutcome::Deferred(NotifyDefer::InProgress));
            }
            let mut sent = false;
            for to in &self.links {
                if exhausted.contains(&to.channel) {
                    quota = true;
                    continue;
                }
                match self.chat(to).await {
                    Ok(()) => sent = true,
                    // The platform refuses Pushes until its quota period
                    // resets, maybe part-way through the batch: email
                    // instead, and leave that Channel alone this call.
                    Err(e) if matches!(e.root(), Error::QuotaExhausted(_)) => {
                        exhausted.insert(to.channel);
                        quota = true;
                    }
                    // What went out is recorded: the retry sends the rest.
                    Err(e @ Error::PartialSend { .. }) => return Err(e),
                    Err(Error::Unreachable | Error::Unsupported(_)) => {}
                    Err(e @ (Error::RateLimited { .. } | Error::Db(_))) => return Err(e),
                    Err(_) => failed = true,
                }
            }
            if sent {
                return finished(d, claim, "chat").await;
            }
        }
        // A cut-off run may have sent the whole chat on a Link that is
        // gone now: that Student has it, so no email on top.
        if claim::chat_on_record(d, &self.key, self.v.chat.len()).await? {
            return finished(d, claim, "chat").await;
        }
        if failed {
            claim.release(d).await?;
            return Ok(NotifyOutcome::Failed(NotifyFailure::Platform));
        }
        self.email(claim, false, quota).await
    }

    /// The batch on one Link, from where its rows say (Channels without
    /// retry keys) or whole with the same key (LINE).
    async fn chat(&self, to: &ChannelAccountId) -> Result<()> {
        let d = self.d;
        let msgs = &self.v.chat;
        let prefix = d.sent_prefix(to, &self.key, msgs).await?;
        if prefix == msgs.len() {
            return Ok(());
        }
        let dedupes = d
            .channel(to.channel.as_str())
            .is_some_and(|c| c.dedupes_keyed_sends());
        let first = if dedupes { 0 } else { prefix };
        let module = Some(self.n.module.as_str());
        d.deliver_from(to, msgs, self.locale, module, &self.key, first)
            .await
            .map(|_| ())
    }

    /// The email, keyed. `again`: a cut-off run may have handed it over.
    /// `quota`: a Link was passed over because its platform's quota is used
    /// up, so without an address the Student waits for the quota to reset.
    async fn email(&self, claim: &Claim, again: bool, quota: bool) -> Result<NotifyOutcome> {
        let d = self.d;
        let sender = &d.services().email;
        let address = self.s.email.clone().filter(|e| !e.trim().is_empty());
        let Some(address) = address else {
            if again {
                return finished(d, claim, "email").await;
            }
            claim.release(d).await?;
            if quota {
                return Ok(NotifyOutcome::Deferred(NotifyDefer::Quota));
            }
            return Ok(NotifyOutcome::Skipped(NotifySkip::NoAddress));
        };
        if again && !sender.dedupes() {
            // At most once: the provider may have it, and cannot say.
            return finished(d, claim, "email").await;
        }
        if !claim.start(d, "email").await? {
            return Ok(NotifyOutcome::Deferred(NotifyDefer::InProgress));
        }
        let email = Email {
            to: address,
            subject: self.v.subject.clone(),
            text: self.v.text.clone(),
            html: None,
        };
        if sender.send_keyed(email, &self.key).await.is_err() {
            claim.release(d).await?;
            return Ok(NotifyOutcome::Failed(NotifyFailure::Email));
        }
        finished(d, claim, "email").await
    }
}

/// `done` by `via`: [`NotifyOutcome::Sent`] / `Emailed`, or `Overtaken`
/// when the claim was lost mid-send (its lease ran out and another call
/// took the Student over; that call's report counts).
async fn finished(d: &Deployment, claim: &Claim, via: &str) -> Result<NotifyOutcome> {
    if !claim.finish(d, via).await? {
        return Ok(NotifyOutcome::Overtaken);
    }
    Ok(earlier(via))
}

async fn load(d: &Deployment, to: &[NotifyTo]) -> Result<Vec<Student>> {
    let (mut ids, mut codes) = (Vec::new(), Vec::new());
    for r in to {
        match r {
            NotifyTo::Id(id) => ids.push(id.0.as_str()),
            NotifyTo::Code(c) => codes.push(c.as_str()),
        }
    }
    // `IN ()` is not valid SQL: an unused list gets a value no row has.
    let none = [""];
    let ids = if ids.is_empty() { &none[..] } else { &ids[..] };
    let codes = if codes.is_empty() {
        &none[..]
    } else {
        &codes[..]
    };
    let mut stmt = Statement::new(format!(
        "SELECT id, student_code, email, locale, status FROM student \
         WHERE id IN ({}) OR student_code IN ({})",
        marks(ids.len()),
        marks(codes.len())
    ));
    for v in ids.iter().chain(codes) {
        stmt = stmt.bind(*v);
    }
    let rows = d.services().db.query(stmt).await?;
    Ok((0..rows.len())
        .map(|i| Student {
            id: text(&rows, i, "id").unwrap_or_default(),
            code: text(&rows, i, "student_code").unwrap_or_default(),
            email: text(&rows, i, "email"),
            locale: Locale::new(text(&rows, i, "locale").unwrap_or_default()),
            left: text(&rows, i, "status").as_deref() == Some("left"),
        })
        .collect())
}

/// Each Student's `active` Links on a Channel this Deployment has.
async fn links(d: &Deployment, ids: &[&str]) -> Result<HashMap<String, Vec<ChannelAccountId>>> {
    let mut out: HashMap<String, Vec<ChannelAccountId>> = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let mut stmt = Statement::new(format!(
        "SELECT l.student_id, ca.channel, ca.external_id FROM link l \
         JOIN channel_account ca ON ca.id = l.channel_account_id \
         WHERE l.status = 'active' AND l.student_id IN ({}) ORDER BY ca.channel",
        marks(ids.len())
    ));
    for id in ids {
        stmt = stmt.bind(*id);
    }
    let rows = d.services().db.query(stmt).await?;
    for i in 0..rows.len() {
        let channel = text(&rows, i, "channel").unwrap_or_default();
        let Some(kind) = d.channel(&channel).map(|c| c.kind()) else {
            continue;
        };
        let account =
            ChannelAccountId::new(kind, text(&rows, i, "external_id").unwrap_or_default());
        out.entry(text(&rows, i, "student_id").unwrap_or_default())
            .or_default()
            .push(account);
    }
    Ok(out)
}
