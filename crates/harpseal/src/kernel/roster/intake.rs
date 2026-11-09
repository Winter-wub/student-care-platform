//! Reads an uploaded file into rows and checks them (#8, #71): the part of
//! an upload before anything is stored. Loads the Students only for a file
//! that is valid, to compare the emails they already use.

use std::collections::HashMap;

use super::file::{self, FoundHeaders, Skipped};
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
    /// The header rows of the sheets whose headers differ (#134).
    pub(super) headers: Vec<FoundHeaders>,
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
    let full = mode == "full";
    let skipped = rows.skipped;
    let with = |result| Read {
        skipped: skipped.clone(),
        result,
        headers: Vec::new(),
    };
    // Students in a hidden roster sheet would become left.
    if full && let Err(p) = hidden_roster_sheets(&skipped) {
        return Ok(with(Err(p)));
    }
    // A file refused for what is in it never needs the Students.
    let valid = match validate::validate(&rows.rows, config) {
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
        match validate::validate_against(&rows.rows, config, &taken) {
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

/// The template's fictional example row (#134), as the catalog writes it
/// in each Locale Staff may download the template in. Matched on code,
/// name and email together: the demo roster's first Student shares the
/// example's code and name, not its email.
pub(super) struct Example {
    code: String,
    names: Vec<String>,
    email: String,
}

impl Example {
    pub(super) fn of(catalog: &crate::manifest::Catalog, config: &DeploymentConfig) -> Self {
        let locales = [config.default_locale.0.as_str(), "th", "en"];
        let t = |l: &str, k: &str| catalog.t(&crate::message::Locale(l.into()), k);
        let mut names: Vec<String> = locales
            .iter()
            .map(|l| t(l, "kernel.upload.example_name"))
            .collect();
        names.dedup();
        Self {
            code: t(&config.default_locale.0, "kernel.upload.example_code"),
            names,
            email: t(&config.default_locale.0, "kernel.upload.example_email"),
        }
    }
}

/// Refuses a file that still holds the template's example row, named by
/// row: it would import a fictional Student.
pub(super) fn refuse_example(read: Read, example: &Example) -> Read {
    let Ok(checked) = &read.result else {
        return read;
    };
    let rows: Vec<Problem> = checked
        .valid
        .records
        .iter()
        .filter(|r| {
            r.code == example.code
                && example.names.contains(&r.fields.display_name)
                && r.fields.email.as_deref() == Some(example.email.as_str())
        })
        .map(|r| Problem::new(r.row, "", "example_row"))
        .collect();
    if rows.is_empty() {
        return read;
    }
    Read {
        result: Err(rows),
        ..read
    }
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
