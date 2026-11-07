//! A Module notifies Students it chose (#73): by chat on every live Link,
//! metered and recorded like any Kernel Push, else by email. The Module
//! picks the Students (no Audience here) and passes at most
//! [`MAX_NOTIFY`] per call; a caller-supplied key makes a retry send
//! nothing twice. The run itself is in `notify/run.rs`.

use std::collections::BTreeMap;

use super::{JobContext, PageState};
use crate::delivery::StudentId;
use crate::message::{Locale, Message};
use crate::{Error, Result};

mod run;

/// Most Students in one [`JobContext::notify`] / [`PageState::notify`]
/// call. Each Student costs one outgoing request per Push request on each
/// live Link (LINE packs up to 5 Messages in one; Telegram sends one per
/// Message) or one email request, so 40 Students with a one-request batch
/// and one Link each stay under the 50 subrequests of a Cloudflare Workers
/// free-plan invocation. Page through a longer list over several Job
/// runs (one child Job per chunk).
pub const MAX_NOTIFY: usize = 40;
/// Longest [`Notice`] key, in bytes.
pub const MAX_KEY: usize = 100;

/// What one Locale's Students get: the chat batch, or the email when they
/// have no live Link (or every Link is unreachable).
#[derive(Debug, Clone)]
struct Version {
    chat: Vec<Message>,
    subject: String,
    text: String,
}

/// A message batch for chosen Students, one version per Locale, under an
/// idempotency key.
///
/// ```
/// # use harpseal::{Locale, Message, Notice};
/// let n = Notice::new("fees", "fees:2026-11-09")
///     .locale(Locale::th(), vec![Message::text("ชำระค่าธรรมเนียม")], "เตือน", "ชำระค่าธรรมเนียม")
///     .locale(Locale::en(), vec![Message::text("Fees are due")], "Reminder", "Fees are due");
/// assert_eq!(n.key(), "fees:2026-11-09");
/// ```
#[derive(Debug, Clone)]
pub struct Notice {
    module: String,
    key: String,
    versions: BTreeMap<Locale, Version>,
}

impl Notice {
    /// A notice from `module` (stored on its Outbound Message rows) with
    /// idempotency `key`: one key names one notice. Calls with the same key
    /// never send a Student the notice twice, so a retried Job (or a
    /// second click) reuses it; a new notice needs a new key (e.g. the
    /// Module name and the week it is for).
    pub fn new(module: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            module: module.into(),
            key: key.into(),
            versions: BTreeMap::new(),
        }
    }

    /// The version for Students in `locale`: the chat batch, and the email
    /// subject and plain text body. A Student whose Locale has no version
    /// gets the Deployment's default Locale's (else is skipped).
    pub fn locale(
        mut self,
        locale: Locale,
        chat: Vec<Message>,
        subject: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        let v = Version {
            chat,
            subject: subject.into(),
            text: text.into(),
        };
        self.versions.insert(locale, v);
        self
    }

    /// The idempotency key.
    pub fn key(&self) -> &str {
        &self.key
    }

    fn check(&self, to: usize) -> Result<()> {
        let bad = |what: &str| Err(Error::Invalid(format!("notice: {what}")));
        if self.key.trim().is_empty() || self.key.len() > MAX_KEY {
            return bad("the key must have 1 to 100 bytes");
        }
        if self.module.trim().is_empty() {
            return bad("no module");
        }
        if self.versions.is_empty() {
            return bad("no Locale version");
        }
        let empty = |v: &Version| v.chat.is_empty() || v.subject.trim().is_empty();
        if self.versions.values().any(empty) {
            return bad("a version has no chat Message or no email subject");
        }
        if to > MAX_NOTIFY {
            return bad("more than MAX_NOTIFY Students");
        }
        Ok(())
    }

    /// The version for `locale`, else for `default`.
    fn version(&self, locale: &Locale, default: &Locale) -> Option<(&Locale, &Version)> {
        self.versions
            .get_key_value(locale)
            .or_else(|| self.versions.get_key_value(default))
    }
}

/// A Student to notify: by primary key or by student code (the roster's
/// student ID).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotifyTo {
    /// `student.id`.
    Id(StudentId),
    /// `student.student_code`.
    Code(String),
}

impl From<StudentId> for NotifyTo {
    fn from(id: StudentId) -> Self {
        NotifyTo::Id(id)
    }
}

impl NotifyTo {
    /// By student code.
    pub fn code(code: impl Into<String>) -> Self {
        NotifyTo::Code(code.into())
    }
}

/// Why a Student got nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotifySkip {
    /// No Student has this id or code.
    Unknown,
    /// The Student `left` the roster.
    Left,
    /// No version in the Student's Locale nor the default one.
    NoText,
    /// No live, reachable Link and no email address.
    NoAddress,
}

/// What happened to one Student.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotifyOutcome {
    /// The chat batch went out on at least one live Link.
    Sent,
    /// Emailed (no live Link, or every one unreachable).
    Emailed,
    /// Nothing sent.
    Skipped(NotifySkip),
    /// The send failed (`platform`: the Channel refused it; `email`: the
    /// email port did). Nothing is recorded as done: a later call with the
    /// same key tries this Student again.
    Failed(&'static str),
}

/// One Student's line in a [`NotifyReport`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Notified {
    /// As passed in.
    pub to: NotifyTo,
    /// The Student, when found.
    pub student: Option<StudentId>,
    /// What happened.
    pub outcome: NotifyOutcome,
    /// `true` when an earlier call with the key did it (nothing sent now).
    pub again: bool,
}

/// What a notify call did, Student by Student (in input order), with
/// totals.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct NotifyReport {
    /// One line per Student passed in.
    pub students: Vec<Notified>,
    /// Students reached by chat (now or earlier with the key).
    pub sent: usize,
    /// Students emailed (now or earlier with the key).
    pub emailed: usize,
    /// Students skipped.
    pub skipped: usize,
    /// Students whose send failed: retry them with the same key.
    pub failed: usize,
}

impl NotifyReport {
    fn push(&mut self, n: Notified) {
        match n.outcome {
            NotifyOutcome::Sent => self.sent += 1,
            NotifyOutcome::Emailed => self.emailed += 1,
            NotifyOutcome::Skipped(_) => self.skipped += 1,
            NotifyOutcome::Failed(_) => self.failed += 1,
        }
        self.students.push(n);
    }
}

impl JobContext {
    /// Sends `notice` to the Students `to` (at most [`MAX_NOTIFY`]): the
    /// chat batch on every live Link (a keyed Push per Link, metered and
    /// recorded like a Kernel send, `module` on the rows), else the email
    /// when the Student has an address. See `docs/guide/deployment.md`
    /// ("Notifying chosen Students").
    ///
    /// `Err` when the notice or the list is invalid (`Error::Invalid`,
    /// nothing sent) or a send must wait (a rate limit, a partial send, a
    /// database error): what went out is recorded, so the Job's retry
    /// with the same notice sends only the rest.
    pub async fn notify(&self, notice: &Notice, to: &[NotifyTo]) -> Result<NotifyReport> {
        run::notify(&self.deployment, notice, to).await
    }
}

impl PageState {
    /// [`JobContext::notify`] from a page (a back-office action). Keep the
    /// list short: the Staff member waits for every send.
    pub async fn notify(&self, notice: &Notice, to: &[NotifyTo]) -> Result<NotifyReport> {
        run::notify(&self.deployment, notice, to).await
    }
}
