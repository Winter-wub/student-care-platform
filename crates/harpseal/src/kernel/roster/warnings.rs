//! Severity of a roster problem and the email checks that look across rows
//! (#71). A problem blocks (the whole file is refused) or warns (the row
//! imports and the preview lists the warning); which warning classes block
//! is `[roster] blocking`, see [`crate::config::WARNING_CLASSES`].

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::store::Existing;
use super::validate::{MAX_PROBLEMS, Problem};
use crate::kernel::students::email;

/// Whether a problem stops the import.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Severity {
    /// The file is refused. What every problem was before #71.
    #[default]
    Blocking,
    /// The row imports; the problem is listed in the preview and report.
    Warning,
}

impl Severity {
    pub(crate) fn is_blocking(&self) -> bool {
        *self == Severity::Blocking
    }

    /// Catalog key of the label.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Severity::Blocking => "kernel.roster.severity_blocking",
            Severity::Warning => "kernel.roster.severity_warning",
        }
    }
}

/// An existing Student that has an email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Owner {
    pub(crate) code: String,
    /// Status `active` now (not `left`).
    pub(crate) active: bool,
    /// Staff set the status by hand: an import keeps it.
    pub(crate) status_held: bool,
    /// Staff set the email by hand: an import keeps it.
    pub(crate) email_held: bool,
}

impl Owner {
    /// Whether this Student still has the email after the import: not
    /// left (a listed Student comes back unless their status is held; an
    /// unlisted one goes in `full` mode unless held), and not given
    /// another by the file.
    fn keeps(&self, listed: bool, full: bool) -> bool {
        let stays_active = match (listed, self.status_held) {
            (true, false) => true,
            (true, true) => self.active,
            (false, held) => self.active && (!full || held),
        };
        stays_active && (self.email_held || !listed)
    }
}

/// What the file is compared with: the emails existing Students will have
/// after the import (trimmed, lowercase, see [`email::key`]).
#[derive(Debug, Default)]
pub(crate) struct Taken {
    by_email: HashMap<String, Vec<Owner>>,
    /// Codes of Students whose email Staff set by hand: the file's value
    /// for them is not stored, so it is not compared.
    hand_set: HashSet<String>,
    full: bool,
}

impl Taken {
    /// The emails of `existing` (by student code) for an import in `full`
    /// mode or not.
    pub(crate) fn of(existing: &HashMap<String, Existing>, full: bool) -> Self {
        let mut out = Self {
            full,
            ..Self::default()
        };
        for (code, e) in existing {
            let email_held = e.overrides.iter().any(|o| o == "email");
            if email_held {
                out.hand_set.insert(code.clone());
            }
            let Some(email) = e.fields.email.as_deref().filter(|m| !m.trim().is_empty()) else {
                continue;
            };
            let owner = Owner {
                code: code.clone(),
                active: e.fields.status == "active",
                status_held: super::store::status_held(e),
                email_held,
            };
            out.by_email
                .entry(email::key(email))
                .or_default()
                .push(owner);
        }
        out
    }

    fn hand_set(&self, code: &str) -> bool {
        self.hand_set.contains(code)
    }

    fn other_owner(&self, key: &str, code: &str, codes: &BTreeSet<&str>) -> bool {
        self.by_email.get(key).is_some_and(|owners| {
            owners
                .iter()
                .any(|o| o.code != code && o.keeps(codes.contains(o.code.as_str()), self.full))
        })
    }
}

/// The valid emails of a file, to find those two Students would share.
#[derive(Debug, Default)]
pub(crate) struct Emails {
    seen: Vec<Seen>,
}

#[derive(Debug)]
struct Seen {
    /// Position of the row among the file's rows.
    index: usize,
    row: usize,
    sheet: String,
    code: String,
    key: String,
}

impl Emails {
    /// Notes the email of the row `(index, number, sheet)`, unless Staff
    /// set that Student's email by hand (the file's value is not stored).
    pub(crate) fn add(&mut self, at: (usize, usize, &str), code: &str, email: &str, taken: &Taken) {
        let (index, row, sheet) = at;
        if taken.hand_set(code) {
            return;
        }
        self.seen.push(Seen {
            index,
            row,
            sheet: sheet.into(),
            code: code.into(),
            key: email::key(email),
        });
    }

    /// `(index, row, sheet)` of every row whose email another Student in
    /// the file has, or an existing Student who keeps theirs.
    pub(crate) fn shared(
        &self,
        taken: &Taken,
        codes: &BTreeSet<&str>,
    ) -> Vec<(usize, usize, &str)> {
        let mut count: HashMap<&str, usize> = HashMap::new();
        for s in &self.seen {
            *count.entry(s.key.as_str()).or_default() += 1;
        }
        self.seen
            .iter()
            .filter(|s| count[s.key.as_str()] > 1 || taken.other_owner(&s.key, &s.code, codes))
            .map(|s| (s.index, s.row, s.sheet.as_str()))
            .collect()
    }
}

/// Rows of the empty optional cells of one column (#71): one warning per
/// sheet and column, not one per cell.
#[derive(Debug, Default)]
pub(crate) struct Blanks {
    columns: Vec<Blank>,
}

#[derive(Debug)]
struct Blank {
    sheet: String,
    column: String,
    rows: Vec<usize>,
    count: u64,
}

/// Rows named in an aggregated warning.
const SAMPLE_ROWS: usize = 5;

impl Blanks {
    pub(crate) fn add(&mut self, sheet: &str, column: &str, row: usize) {
        let at = self
            .columns
            .iter()
            .position(|b| b.sheet == sheet && b.column == column);
        let b = match at {
            Some(i) => &mut self.columns[i],
            None => {
                self.columns.push(Blank {
                    sheet: sheet.into(),
                    column: column.into(),
                    rows: Vec::new(),
                    count: 0,
                });
                self.columns.last_mut().expect("just pushed")
            }
        };
        b.count += 1;
        if b.rows.len() < SAMPLE_ROWS {
            b.rows.push(row);
        }
    }

    /// One `empty_attribute` problem per column, at its first row.
    pub(crate) fn problems(self, blocks: bool) -> Vec<Problem> {
        self.columns
            .into_iter()
            .map(|b| {
                let mut p = Problem::new(b.rows[0], &b.column, "empty_attribute").on(&b.sheet);
                p.count = b.count;
                p.rows = b.rows;
                p.with_severity(blocks)
            })
            .collect()
    }
}

/// How many problems each class (reason code) has.
pub(crate) fn classes(problems: &[Problem]) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for p in problems {
        *out.entry(p.reason.clone()).or_default() += p.weight();
    }
    out
}

/// How many cells or rows the problems stand for (see [`Problem::weight`]).
pub(crate) fn total(problems: &[Problem]) -> u64 {
    problems.iter().map(Problem::weight).sum()
}

/// The problems a report keeps: at most [`MAX_PROBLEMS`], in file order.
pub(crate) fn kept(problems: Vec<Problem>) -> Vec<Problem> {
    problems.into_iter().take(MAX_PROBLEMS).collect()
}

#[cfg(test)]
#[path = "warnings_tests.rs"]
mod tests;
