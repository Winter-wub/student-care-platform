//! One notify call (#73): load the Students, their live Links and what
//! earlier calls with the key did, then per Student chat or email.
//!
//! Idempotency, per Student:
//! - a `notify_delivery` row `done` (or an email row left `sending`):
//!   reported again, nothing sent (email at most once);
//! - chat: each Link's batch key is `notify:<key>:<student id>`; the
//!   Messages its `outbound_message` rows record as wholly sent are not
//!   sent again (`Deployment::sent_prefix`), the rest go keyed (LINE
//!   de-duplicates a request whose answer was lost).

use std::collections::HashMap;

use super::{Notice, Notified, NotifyOutcome, NotifyReport, NotifySkip, NotifyTo, Version};
use crate::delivery::{ChannelAccountId, StudentId};
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

pub(super) async fn notify(d: &Deployment, n: &Notice, to: &[NotifyTo]) -> Result<NotifyReport> {
    n.check(to.len())?;
    let mut report = NotifyReport::default();
    if to.is_empty() {
        return Ok(report);
    }
    let students = load(d, to).await?;
    let ids: Vec<&str> = students.iter().map(|s| s.id.as_str()).collect();
    let links = links(d, &ids).await?;
    let mut done = recorded(d, n, &ids).await?;
    let default = d.config().default_locale.clone();
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
        if let Some(outcome) = done.get(&s.id).and_then(Row::finished) {
            report.push(line(Some(s), outcome, true));
            continue;
        }
        let outcome = if s.left {
            NotifyOutcome::Skipped(NotifySkip::Left)
        } else if let Some((locale, v)) = n.version(&s.locale, &default) {
            let mine = links.get(&s.id).map_or(&[][..], Vec::as_slice);
            one(d, n, s, locale, v, mine).await?
        } else {
            NotifyOutcome::Skipped(NotifySkip::NoText)
        };
        if matches!(outcome, NotifyOutcome::Sent | NotifyOutcome::Emailed) {
            // A duplicate of this Student later in `to` reports it again.
            let via = if outcome == NotifyOutcome::Sent {
                "chat"
            } else {
                "email"
            };
            done.insert(s.id.clone(), Row::done(via));
        }
        report.push(line(Some(s), outcome, false));
    }
    Ok(report)
}

/// Chat on every live Link, else email.
async fn one(
    d: &Deployment,
    n: &Notice,
    s: &Student,
    locale: &Locale,
    v: &Version,
    links: &[ChannelAccountId],
) -> Result<NotifyOutcome> {
    let db = &d.services().db;
    let mut failed = None;
    if !links.is_empty() {
        db.execute(upsert(d, n, s, "chat", false)?).await?;
        let key = format!("notify:{}:{}", n.key, s.id);
        let mut sent = false;
        for to in links {
            let first = d.sent_prefix(to, &key, &v.chat).await?;
            let module = Some(n.module.as_str());
            match d
                .deliver_from(to, &v.chat, locale, module, &key, first)
                .await
            {
                Ok(_) => sent = true,
                // What went out is recorded: the retry sends the rest.
                Err(e @ Error::PartialSend { .. }) => return Err(e),
                Err(Error::Unreachable | Error::Unsupported(_)) => {}
                Err(e @ (Error::RateLimited { .. } | Error::Db(_))) => return Err(e),
                Err(_) => failed = Some("platform"),
            }
        }
        if sent {
            db.execute(finish(n, s, "chat")).await?;
            return Ok(NotifyOutcome::Sent);
        }
        if let Some(why) = failed {
            return Ok(NotifyOutcome::Failed(why));
        }
    }
    let Some(address) = s.email.clone().filter(|e| !e.trim().is_empty()) else {
        return Ok(NotifyOutcome::Skipped(NotifySkip::NoAddress));
    };
    // Claimed before the send: a run cut off after it never emails twice.
    db.execute(upsert(d, n, s, "email", true)?).await?;
    let email = Email {
        to: address,
        subject: v.subject.clone(),
        text: v.text.clone(),
        html: None,
    };
    if d.services().email.send(email).await.is_err() {
        let forget = Statement::new(
            "DELETE FROM notify_delivery WHERE notify_key = ? AND student_id = ? \
             AND status = 'sending'",
        )
        .bind(n.key.as_str())
        .bind(s.id.as_str());
        let _ = db.execute(forget).await;
        return Ok(NotifyOutcome::Failed("email"));
    }
    db.execute(finish(n, s, "email")).await?;
    Ok(NotifyOutcome::Emailed)
}

/// The `sending` row: inserted, or (`replace`) turned to `via`.
fn upsert(d: &Deployment, n: &Notice, s: &Student, via: &str, replace: bool) -> Result<Statement> {
    let now = d.services().now_ms();
    let on_conflict = if replace {
        "DO UPDATE SET via = excluded.via, status = 'sending', updated_at = excluded.updated_at"
    } else {
        "DO NOTHING"
    };
    Ok(Statement::new(format!(
        "INSERT INTO notify_delivery (notify_key, student_id, module, via, status, \
         created_at, updated_at) VALUES (?, ?, ?, ?, 'sending', ?, ?) \
         ON CONFLICT (notify_key, student_id) {on_conflict}"
    ))
    .bind(n.key.as_str())
    .bind(s.id.as_str())
    .bind(n.module.as_str())
    .bind(via)
    .bind(now)
    .bind(now))
}

fn finish(n: &Notice, s: &Student, via: &str) -> Statement {
    Statement::new(
        "UPDATE notify_delivery SET via = ?, status = 'done' WHERE notify_key = ? \
         AND student_id = ?",
    )
    .bind(via)
    .bind(n.key.as_str())
    .bind(s.id.as_str())
}

/// A `notify_delivery` row.
struct Row {
    via: String,
    done: bool,
}

impl Row {
    fn done(via: &str) -> Self {
        Row {
            via: via.into(),
            done: true,
        }
    }

    /// What an earlier call finished: done, or an email it claimed.
    fn finished(&self) -> Option<NotifyOutcome> {
        match (self.via.as_str(), self.done) {
            ("chat", true) => Some(NotifyOutcome::Sent),
            ("email", _) => Some(NotifyOutcome::Emailed),
            _ => None,
        }
    }
}

async fn recorded(d: &Deployment, n: &Notice, ids: &[&str]) -> Result<HashMap<String, Row>> {
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let mut stmt = Statement::new(format!(
        "SELECT student_id, via, status FROM notify_delivery WHERE notify_key = ? \
         AND student_id IN ({})",
        marks(ids.len())
    ))
    .bind(n.key.as_str());
    for id in ids {
        stmt = stmt.bind(*id);
    }
    let rows = d.services().db.query(stmt).await?;
    for i in 0..rows.len() {
        let row = Row {
            via: text(&rows, i, "via").unwrap_or_default(),
            done: text(&rows, i, "status").as_deref() == Some("done"),
        };
        out.insert(text(&rows, i, "student_id").unwrap_or_default(), row);
    }
    Ok(out)
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
