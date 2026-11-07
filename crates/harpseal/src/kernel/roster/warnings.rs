//! Severity of a roster problem and the email checks that look across rows
//! (#71). A problem blocks (the whole file is refused) or warns (the row
//! imports and the preview lists the warning); which warning classes block
//! is `[roster] blocking`, see [`crate::config::WARNING_CLASSES`].

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use super::store::Existing;
use super::validate::{MAX_PROBLEMS, Problem};

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
    /// Staff edited the email by hand, so the file will not change it.
    pub(crate) held: bool,
}

/// Existing Students' emails (trimmed, lowercase) and who has them.
pub(crate) type Taken = HashMap<String, Vec<Owner>>;

/// The key two emails are compared by.
pub(crate) fn key(email: &str) -> String {
    email.trim().to_lowercase()
}

/// The emails of `existing` (by student code).
pub(crate) fn taken(existing: &HashMap<String, Existing>) -> Taken {
    let mut out = Taken::new();
    for (code, e) in existing {
        let Some(email) = e.fields.email.as_deref().filter(|m| !m.trim().is_empty()) else {
            continue;
        };
        out.entry(key(email)).or_default().push(Owner {
            code: code.clone(),
            held: e.overrides.iter().any(|o| o == "email"),
        });
    }
    out
}

/// The valid emails of a file, to find those two Students would share.
#[derive(Debug, Default)]
pub(crate) struct Emails {
    seen: Vec<Seen>,
}

#[derive(Debug)]
struct Seen {
    row: usize,
    sheet: String,
    code: String,
    key: String,
}

impl Emails {
    pub(crate) fn add(&mut self, row: usize, sheet: &str, code: &str, email: &str) {
        self.seen.push(Seen {
            row,
            sheet: sheet.into(),
            code: code.into(),
            key: key(email),
        });
    }

    /// `(row, sheet)` of every row whose email another Student in the file
    /// has, or an existing Student outside the file does (or inside it,
    /// when Staff set that email by hand: the file will not change it).
    pub(crate) fn shared(&self, taken: &Taken, codes: &BTreeSet<&str>) -> Vec<(usize, &str)> {
        let mut count: HashMap<&str, usize> = HashMap::new();
        for s in &self.seen {
            *count.entry(s.key.as_str()).or_default() += 1;
        }
        let other_owner = |s: &Seen| {
            taken.get(&s.key).is_some_and(|owners| {
                owners
                    .iter()
                    .any(|o| o.code != s.code && (o.held || !codes.contains(o.code.as_str())))
            })
        };
        self.seen
            .iter()
            .filter(|s| count[s.key.as_str()] > 1 || other_owner(s))
            .map(|s| (s.row, s.sheet.as_str()))
            .collect()
    }
}

/// How many problems each class (reason code) has.
pub(crate) fn classes(problems: &[Problem]) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for p in problems {
        *out.entry(p.reason.clone()).or_default() += 1;
    }
    out
}

/// The problems a report keeps: blocking ones first, then warnings, at most
/// [`MAX_PROBLEMS`], each group in file order.
pub(crate) fn kept(problems: Vec<Problem>) -> Vec<Problem> {
    let (blocking, warned): (Vec<_>, Vec<_>) =
        problems.into_iter().partition(|p| p.severity.is_blocking());
    blocking
        .into_iter()
        .chain(warned)
        .take(MAX_PROBLEMS)
        .collect()
}

#[cfg(test)]
#[path = "warnings_tests.rs"]
mod tests;
