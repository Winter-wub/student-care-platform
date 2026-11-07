//! Reads an uploaded file into rows and checks them (#8, #71): the part of
//! an upload before anything is stored. Needs the Students for one check,
//! the emails they already use, so it is the only async step of validation.

use std::collections::HashMap;

use super::file::{self, Skipped};
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
    /// The checked file, or every problem found (warnings included).
    pub(super) result: core::result::Result<Checked, Vec<Problem>>,
}

/// Reads and validates `bytes` for an import in `mode`.
pub(super) async fn read(
    s: &Services,
    config: &DeploymentConfig,
    name: &str,
    bytes: &[u8],
    mode: &str,
) -> Result<Read> {
    let refused = |problems| Read {
        skipped: Vec::new(),
        result: Err(problems),
    };
    if bytes.len() > MAX_FILE_BYTES {
        return Ok(refused(vec![problem("too_large")]));
    }
    let rows = match file::read(bytes, name, &config.roster) {
        Ok(r) => r,
        Err(e) => return Ok(refused(validate::file_problems(&e))),
    };
    let full = mode == "full";
    let skipped = rows.skipped;
    let with = |result| Read {
        skipped: skipped.clone(),
        result,
    };
    // Students in a hidden roster sheet would become left.
    if full && let Err(p) = hidden_roster_sheets(&skipped) {
        return Ok(with(Err(p)));
    }
    let existing = store::all_students(s).await?;
    let taken = warnings::taken(&existing);
    let valid = match validate::validate_against(&rows.rows, config, &taken) {
        Ok(v) => v,
        Err(p) => return Ok(with(Err(p))),
    };
    // A full file with no rows would mark everyone left.
    if full && valid.records.is_empty() {
        return Ok(with(Err(vec![problem("empty_full")])));
    }
    Ok(with(Ok(Checked { valid, existing })))
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
