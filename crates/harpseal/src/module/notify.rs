//! A Module sends a Notice to Students it chose (#73): by chat on every
//! live Link, metered and recorded like any Kernel Push, else by email.
//! The Module picks the Students (no Audience here) and passes at most
//! [`MAX_NOTIFY`] per call; its Module name and key make a retry, or a
//! second call at the same time, send nothing twice. The run is in
//! `notify/run.rs`, the per-Student claim in `notify/claim.rs`.

use std::collections::BTreeMap;

use super::{JobContext, PageState};
use crate::delivery::{ChannelKind, StudentId};
use crate::message::{Locale, Message};
use crate::{Error, Result};

mod claim;
mod run;

/// Most Students in one [`JobContext::notify`] / [`PageState::notify`]
/// call (each one is a bound parameter of a few queries; D1 allows 100).
/// What one call may spend on outgoing requests is [`Notice::budget`].
pub const MAX_NOTIFY: usize = 40;
/// The default [`Notice::budget`]: outgoing requests (Workers
/// subrequests) one call may make. 40 leaves room under the 50 of a
/// Cloudflare Workers free-plan invocation for the run's other calls; a
/// paid plan allows 10,000 per invocation.
pub const DEFAULT_BUDGET: usize = 40;
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

impl Version {
    /// Outgoing requests one Link may cost in the worst case: one per
    /// Message, one per Card of a Card List, one per 4096 characters of a
    /// text (Telegram; LINE packs up to 5 objects per request, so it
    /// spends less).
    fn requests(&self) -> usize {
        self.chat
            .iter()
            .map(|m| match m {
                Message::CardList(l) => l.0.len().max(1),
                Message::Text(t) => t.chars().count().div_ceil(4096).max(1),
                _ => 1,
            })
            .sum()
    }
}

/// A message batch for chosen Students, one version per Locale, under a
/// Module's idempotency key.
///
/// ```
/// # use harpseal::{Locale, Message, Notice};
/// let n = Notice::new("fees", "2026-11-09")
///     .locale(Locale::th(), vec![Message::text("ชำระค่าธรรมเนียม")], "เตือน", "ชำระค่าธรรมเนียม")
///     .locale(Locale::en(), vec![Message::text("Fees are due")], "Reminder", "Fees are due");
/// assert_eq!(n.key(), "2026-11-09");
/// ```
#[derive(Debug, Clone)]
pub struct Notice {
    module: String,
    key: String,
    versions: BTreeMap<Locale, Version>,
    budget: usize,
}

impl Notice {
    /// A Notice from `module` (stored on its Outbound Message rows; no
    /// `:`) with idempotency `key`. The Kernel scopes the key by `module`:
    /// one (module, key) names one Notice, and calls with it never send a
    /// Student the Notice twice, so a retried Job or a second click
    /// reuses it; a new Notice needs a new key (e.g. the week it is for).
    /// Keep a key's texts the same: an email retried under the key with
    /// another body is refused by Resend (409 `invalid_idempotent_request`,
    /// within its 24 hours), which comes back as
    /// [`NotifyOutcome::Failed`] (`Email`).
    pub fn new(module: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            module: module.into(),
            key: key.into(),
            versions: BTreeMap::new(),
            budget: DEFAULT_BUDGET,
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

    /// The outgoing requests one call may make (default
    /// [`DEFAULT_BUDGET`]). Before each Student the Kernel adds their
    /// worst case (every live Link's requests for the batch, plus one
    /// email when they have an address); a Student who would go over is
    /// [`NotifyDefer::Budget`], as is everyone after: call again with them.
    /// The first Student always goes, whatever their cost.
    pub fn budget(mut self, requests: usize) -> Self {
        self.budget = requests;
        self
    }

    /// The idempotency key, as given.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The key as stored: `<module>:<key>`.
    fn scoped(&self) -> String {
        format!("{}:{}", self.module, self.key)
    }

    fn check(&self, to: usize) -> Result<()> {
        let bad = |what: &str| Err(Error::Invalid(format!("notice: {what}")));
        if self.key.trim().is_empty() || self.key.len() > MAX_KEY {
            return bad("the key must have 1 to 100 bytes");
        }
        let m = &self.module;
        if m.trim().is_empty() || m.len() > 64 || m.contains(':') {
            return bad("the module must have 1 to 64 bytes and no `:`");
        }
        if self.versions.is_empty() {
            return bad("no Locale version");
        }
        let empty = |v: &Version| v.chat.is_empty() || v.subject.trim().is_empty();
        if self.versions.values().any(empty) {
            return bad("a version has no chat Message or no email subject");
        }
        if self.budget == 0 {
            return bad("a budget of 0 requests");
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

/// Why a Student got nothing, for good.
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

/// Why a Student was not tried in this call: call again with the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotifyDefer {
    /// Over the call's [`Notice::budget`].
    Budget,
    /// Another call with the key is sending to this Student now (or was
    /// cut off less than 10 minutes ago).
    InProgress,
    /// Their only way is a Channel whose platform refuses Pushes until its
    /// own quota resets ([`Error::QuotaExhausted`]), and they have no
    /// email address: call again once the quota has reset (LINE: the
    /// next month).
    Quota,
}

/// Why a send failed. Nothing is recorded as done: a later call with the
/// key tries the Student again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotifyFailure {
    /// A Channel refused the Push (not unreachable, not a rate limit).
    Platform,
    /// The email port refused the email.
    Email,
}

/// What happened to one Student.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotifyOutcome {
    /// The chat batch went out on at least one live Link.
    Sent,
    /// Emailed (no live Link, every one unreachable, or its Channel's
    /// platform quota used up, even part-way through the batch).
    Emailed,
    /// Nothing sent, and nothing will be.
    Skipped(NotifySkip),
    /// Not tried in this call.
    Deferred(NotifyDefer),
    /// The send failed.
    Failed(NotifyFailure),
    /// Sent, but this call's claim ran out mid-send and another call took
    /// the Student over: that call's report is the one that counts (it
    /// finds the chat on record, or resends the email under its key).
    Overtaken,
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
    /// Students skipped for good.
    pub skipped: usize,
    /// Students not tried: call again with them and the key.
    pub deferred: usize,
    /// Students whose send failed: call again with them and the key.
    pub failed: usize,
    /// Students another call took over mid-send ([`NotifyOutcome::Overtaken`]).
    pub overtaken: usize,
    /// Channels whose platform refused a Push because its own quota is
    /// used up ([`Error::QuotaExhausted`]); not called again in this call.
    pub exhausted: Vec<ChannelKind>,
}

impl NotifyReport {
    fn push(&mut self, n: Notified) {
        match n.outcome {
            NotifyOutcome::Sent => self.sent += 1,
            NotifyOutcome::Emailed => self.emailed += 1,
            NotifyOutcome::Skipped(_) => self.skipped += 1,
            NotifyOutcome::Deferred(_) => self.deferred += 1,
            NotifyOutcome::Failed(_) => self.failed += 1,
            NotifyOutcome::Overtaken => self.overtaken += 1,
        }
        self.students.push(n);
    }
}

impl JobContext {
    /// Sends `notice` to the Students `to` (at most [`MAX_NOTIFY`]): the
    /// chat batch on every live Link (a keyed Push per Link, metered and
    /// recorded like a Kernel send, the Module on the rows), else the
    /// email when the Student has an address. See
    /// `docs/guide/notify.md`.
    ///
    /// `Err` when the notice or the list is invalid (`Error::Invalid`,
    /// nothing sent) or a send must wait (a rate limit, a partial send, a
    /// database error): what went out is recorded, so a retry with the
    /// same notice sends only the rest.
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
