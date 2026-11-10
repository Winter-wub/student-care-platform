//! Reads an uploaded file into rows and checks them (#8, #71): the part of
//! an upload before anything is stored. Loads the Students only for a file
//! that is valid, to compare the emails they already use.

use std::collections::HashMap;

use super::file::{self, Absent, FoundHeaders, Skipped};
use super::preview::MAX_FILE_BYTES;
use super::store::{self, Existing};
use super::validate::{self, Problem, Valid};
use super::warnings;
use crate::config::DeploymentConfig;
use crate::{Result, Services};

/// A valid file with the Students it is compared with.
pub(super) struct Checked {
    pub(super) valid: Valid,
    /// Every Student, by student code.
    pub(super) existing: HashMap<String, Existing>,
}

/// What reading an upload found.
pub(super) struct Read {
    /// Rows and sheets left out (also when the file is refused later).
    pub(super) skipped: Vec<Skipped>,
    /// The may-be-empty columns a sheet leaves out (#154).
    pub(super) absent: Vec<Absent>,
    /// The checked file, or every problem found (warnings included).
    pub(super) result: core::result::Result<Checked, Vec<Problem>>,
    /// The header rows of the sheets whose headers differ (#134).
    pub(super) headers: Vec<FoundHeaders>,
}

/// Reads and validates `bytes` for an import in `mode`; a file that still
/// holds the template's `example` row is refused (#134).
pub(super) async fn read(
    s: &Services,
    config: &DeploymentConfig,
    name: &str,
    bytes: &[u8],
    mode: &str,
    example: &Example,
) -> Result<Read> {
    let refused = |problems| Read {
        skipped: Vec::new(),
        absent: Vec::new(),
        result: Err(problems),
        headers: Vec::new(),
    };
    if bytes.len() > MAX_FILE_BYTES {
        return Ok(refused(vec![problem("too_large")]));
    }
    let rows = match file::read(bytes, name, &config.roster) {
        Ok(r) => r,
        Err(e) => {
            return Ok(Read {
                headers: e.found_headers(),
                ..refused(validate::file_problems(&e))
            });
        }
    };
    // On the rows as read, before validation: a code pattern that refuses
    // the example's code still names the row for what it is.
    let examples = example.rows(&rows.rows, &config.roster);
    let full = mode == "full";
    let skipped = rows.skipped;
    let absent = rows.absent;
    let with = |result| Read {
        skipped: skipped.clone(),
        absent: absent.clone(),
        result: with_examples(&examples, result),
        headers: Vec::new(),
    };
    // Students in a hidden roster sheet would become left.
    if full && let Err(p) = hidden_roster_sheets(&skipped) {
        return Ok(with(Err(p)));
    }
    // A file refused for what is in it never needs the Students.
    let none = warnings::Taken::default();
    let valid = match validate::validate_in(&rows.rows, config, &none, &absent) {
        Ok(v) => v,
        Err(p) => return Ok(with(Err(p))),
    };
    // Now the emails Students already have: the file may only warn, or be
    // refused if the Deployment makes `duplicate_email` blocking.
    let existing = store::all_students(s).await?;
    let taken = warnings::Taken::of(&existing, full);
    let valid = if existing.is_empty() {
        valid
    } else {
        match validate::validate_in(&rows.rows, config, &taken, &absent) {
            Ok(v) => v,
            Err(p) => return Ok(with(Err(p))),
        }
    };
    // A full file with no rows would mark everyone left.
    if full && valid.records.is_empty() {
        return Ok(with(Err(vec![problem("empty_full")])));
    }
    Ok(with(Ok(Checked { valid, existing })))
}

/// The template's fictional example row (#134): its code, name and email
/// as the catalog writes them in each Locale the template may be
/// downloaded in (the Staff member's). Matched on all three together:
/// the demo roster's first Student shares the example's code and name,
/// not its email.
#[derive(Debug, Default)]
pub(super) struct Example {
    rows: Vec<(String, String, String)>,
}

impl Example {
    pub(super) fn of(catalog: &crate::manifest::Catalog, config: &DeploymentConfig) -> Self {
        let mut rows = Vec::new();
        for l in [config.default_locale.0.as_str(), "th", "en"] {
            let t = |k: &str| catalog.t(&crate::message::Locale(l.into()), k);
            let row = (
                t("kernel.upload.example_code"),
                t("kernel.upload.example_name"),
                t("kernel.upload.example_email"),
            );
            if !rows.contains(&row) {
                rows.push(row);
            }
        }
        Self { rows }
    }

    /// One `example_row` problem per row of `read` that is the example.
    fn rows(&self, read: &[file::Row], roster: &crate::config::RosterConfig) -> Vec<Problem> {
        read.iter()
            .filter(|r| {
                let get = |c: &str| r.get(c).unwrap_or_default();
                let cells = (
                    get(&roster.student_code),
                    get(&roster.display_name),
                    get(&roster.email),
                );
                self.rows
                    .iter()
                    .any(|(c, n, e)| (c.as_str(), n.as_str(), e.as_str()) == cells)
            })
            .map(|r| Problem::new(r.number(), "", "example_row").on(r.sheet()))
            .collect()
    }
}

/// `result` refused when the file holds the example row, with the file's
/// own problems (or warnings) after it, so the report keeps them.
fn with_examples(
    examples: &[Problem],
    result: core::result::Result<Checked, Vec<Problem>>,
) -> core::result::Result<Checked, Vec<Problem>> {
    if examples.is_empty() {
        return result;
    }
    let rest = match result {
        Ok(c) => c.valid.warnings,
        Err(p) => p,
    };
    Err(examples.iter().cloned().chain(rest).collect())
}

/// Refuses a `full` file with a hidden sheet holding the roster header:
/// one problem per such sheet.
fn hidden_roster_sheets(skipped: &[Skipped]) -> core::result::Result<(), Vec<Problem>> {
    let problems: Vec<Problem> = skipped
        .iter()
        .filter(|s| s.reason == "hidden_roster_sheet")
        .map(|s| problem("hidden_roster_sheet").on(&s.sheet))
        .collect();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// A problem of the file as a whole.
fn problem(reason: &str) -> Problem {
    Problem::new(0, "", reason)
}
