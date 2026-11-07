//! The roster import (issue #24, contract in #8).
//!
//! Staff upload a CSV or XLSX file with a mode (`full`: the file is the
//! whole roster, Students missing from it become `left`; `upsert`: add and
//! update only). The request stores the file in the FileStore under
//! `roster/<import id>/<file name>`, reads it ([`file`]: the headers must
//! match `[roster]` exactly), validates every row ([`validate`]: one error
//! refuses the whole file, with a downloadable report) and previews what
//! will change ([`preview`]). Confirming queues the Job
//! `kernel.roster.import` ([`job`]), which applies 500 rows per run in one
//! `Db::batch` each and, in `full` mode, ends with the `left` transition
//! ([`finish`]). Only one import is `previewed` / `queued` / `running` at a
//! time. Manual edits win (`students::merge_import`); `roster` Tags are
//! replaced, `manual` Tags kept.
//!
//! Pages (Kernel back-office routes under `/admin/roster`):
//!
//! | Route | Permission |
//! |---|---|
//! | `GET /admin/roster` (imports, upload form, headers) | `kernel.roster.view` |
//! | `GET /admin/roster/template.csv` | `kernel.roster.view` |
//! | `POST /admin/roster/upload` (multipart) | `kernel.roster.import` |
//! | `GET /admin/roster/{id}` (preview, progress, result) | `kernel.roster.view` |
//! | `POST /admin/roster/{id}/confirm`, `…/cancel`, `…/fail` (stuck) | `kernel.roster.import` |
//! | `GET /admin/roster/{id}/report.csv` | `kernel.roster.view` |
//! | `GET /admin/roster/{id}/file` (audited `roster.file.download`) | `kernel.roster.view` |
//!
//! Audit: `roster.import.upload` (each upload), `roster.import` (confirm:
//! counts, file hash, mode, file name), `roster.import.cancel`,
//! `roster.import.fail`, `roster.file.download`, `roster.file.prune`
//! (actor `system`, a year after upload, see [`prune`]); the Job writes `student.status` per Student it
//! marks left and `link.revoke` per Link.
//!
//! From code: `harpseal::roster::{upload, confirm}` ([`api`]) take the same
//! path, checking `kernel.roster.import` themselves.

pub mod api;
mod chunk;
pub(crate) use crate::sheet::csv;
pub(crate) mod file;
mod finish;
mod job;
mod page_import;
mod page_list;
pub(crate) mod preview;
mod prune;
mod rows;
mod service;
mod skipped;
mod store;
mod stuck;
mod texts;
mod texts_sheets;
pub(crate) mod validate;
mod view;

use axum::routing::{get, post};

use crate::config::RosterConfig;
use crate::manifest::JobSpec;
use crate::module::{ModuleRegistrar, UploadSpec};
use view::{IMPORT, VIEW};

/// Largest body `POST /admin/roster/upload` reads: a file of
/// [`preview::MAX_FILE_BYTES`] plus the form's other fields. Only that
/// route gets it (#46); the guard reads other multipart bodies up to
/// `auth::MULTIPART_LIMIT`.
const UPLOAD_LIMIT: usize = preview::MAX_FILE_BYTES + 64 * 1024;

pub(crate) use prune::prune_files;

/// Whether uploads are possible: the Deployment maps roster files.
fn has_mapping(roster: &RosterConfig) -> bool {
    roster.is_mapped()
}

/// Registers the pages and the Job on the Kernel.
pub(crate) fn register(r: &mut ModuleRegistrar) {
    let upload = UploadSpec {
        path: "/roster/upload",
        max_body: UPLOAD_LIMIT,
        too_large: Some((view::BASE, "kernel.roster.reason_too_large")),
    };
    r.messages(texts::DEFAULTS)
        .messages(texts_sheets::DEFAULTS)
        .back_office_page(
            VIEW,
            "/roster",
            "kernel.roster.title",
            20,
            get(page_list::page),
        )
        .back_office(VIEW, "/roster/template.csv", get(page_list::template))
        .back_office_upload(IMPORT, upload, post(page_list::upload))
        .back_office(VIEW, "/roster/{id}", get(page_import::page))
        .back_office(IMPORT, "/roster/{id}/confirm", post(page_import::confirm))
        .back_office(IMPORT, "/roster/{id}/cancel", post(page_import::cancel))
        .back_office(IMPORT, "/roster/{id}/fail", post(page_import::fail))
        .back_office(VIEW, "/roster/{id}/report.csv", get(page_import::report))
        .back_office(VIEW, "/roster/{id}/file", get(page_import::file))
        .job(JobSpec::new(service::JOB_KIND), job::run);
}

/// The `[roster]` the unit tests use (the example Deployment's mapping).
#[cfg(test)]
pub(crate) fn test_config() -> crate::DeploymentConfig {
    crate::DeploymentConfig::from_toml(
        r#"
name = "Seal Bay University (roster)"
default_locale = "th"

[student]
id_pattern = '########'

[roster]
student_code = "รหัสนักศึกษา"
display_name = "ชื่อ-สกุล"
email = "อีเมล"
locale = { column = "ภาษา", default = "th" }
tags = { column = "กลุ่ม", separator = ";" }

[roster.attributes.programme]
column = "หลักสูตร"
type = "enum"
values = ["MSC", "MSC-Intl"]
order = 1

[roster.attributes.year]
column = "ชั้นปี"
type = "integer"
order = 2

[roster.attributes.gpa]
column = "เกรดเฉลี่ย"
type = "number"
optional = true
order = 3
"#,
    )
    .expect("the roster test config parses")
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_roster_section_marks_the_mapping() {
        let base = "name = \"X\"\ndefault_locale = \"th\"\n";
        let none = crate::DeploymentConfig::from_toml(base).unwrap();
        assert!(!none.roster.is_mapped());
        let some =
            crate::DeploymentConfig::from_toml(&format!("{base}[roster]\nstudent_code = \"รหัส\""))
                .unwrap();
        assert!(some.roster.is_mapped());
        let empty = crate::DeploymentConfig::from_toml(&format!("{base}[roster]\n")).unwrap();
        assert!(
            empty.roster.is_mapped(),
            "an empty section still maps the defaults"
        );
        assert!(
            crate::DeploymentConfig::from_toml(&format!("{base}[roster]\ndeclared = true"))
                .is_err()
        );
    }
}
