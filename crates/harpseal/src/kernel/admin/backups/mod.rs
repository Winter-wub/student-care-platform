//! Backups in the back office (#78): the nightly Job that writes a backup
//! (format 2, ADR-0008) to the FileStore, its retention, failure emails,
//! the Admin page that lists and downloads them, and "Download all data"
//! as an Excel workbook.
//!
//! | What | Where |
//! |---|---|
//! | Page, `GET /admin/backups` | `kernel.backup.manage`, nav order 90 |
//! | "Back up now", `POST /admin/backups/now` | same; audited `backup.request` |
//! | One backup as a `.tar`, `GET /admin/backups/{id}/download` | `kernel.backup.download`; streamed, audited `backup.download` |
//! | All data as `.xlsx`, `GET /admin/backups/export.xlsx` (operational tables left out) | `kernel.backup.download`; built in the request, audited `data.export` |
//! | One table as `.xlsx`, `GET /admin/backups/export/{table}` | same |
//! | Job `kernel.backup.run` | on `[backup] schedule` when `enabled` |
//!
//! **Where.** Each backup is the FileStore objects under
//! `backups/<UTC time>/` (`backups/20261107T190000000Z/manifest.json`, …):
//! on R2 in the Worker's bucket, natively in the FileStore directory
//! (`HARPSEAL_FILES`), where `backups/<UTC time>` is a directory
//! `harpseal restore` reads as it is. A downloaded `.tar` unpacks to the
//! same directory. The `backup_run` table indexes them (the FileStore port
//! cannot list keys).
//!
//! **Steps.** One run of the Job dumps tables up to a budget (rows, bytes,
//! objects), saves where it stands as `backups/<UTC time>.progress.json`
//! and enqueues the next step, so a large Deployment is backed up over a
//! few minutes without hitting a Worker's limits; see
//! [`crate::backup`] for what the dump promises while writes continue.
//!
//! **Failures.** A step that fails is retried like any Job (1, 5, 30
//! minutes). When the last retry fails, or a run makes no progress for two
//! hours (a Worker killed mid-step), the run is marked `failed`, the page
//! shows it, and every active Administrator gets one email per failure
//! day. Retention runs only after a successful backup: it deletes runs
//! (done or failed) started more than `[backup] retention_days` ago, with
//! their objects, never the newest successful one.

mod download;
mod export;
mod notify;
mod page;
mod prune;
mod run;
mod store;
mod texts;

use axum::routing::{get, post};

use crate::BuildError;
use crate::config::DeploymentConfig;
use crate::jobs::cron::{self, Schedule};
use crate::manifest::JobSpec;
use crate::module::ModuleRegistrar;

/// The Job kind.
pub(crate) const KIND: &str = "kernel.backup.run";
/// Permission of the page and "back up now".
pub(crate) const MANAGE: &str = "kernel.backup.manage";
/// Permission of the downloads: a backup holds password hashes and every
/// Student's data, so this is Administrator-equivalent.
pub(crate) const DOWNLOAD: &str = "kernel.backup.download";
/// The page, under `/admin`.
const PATH: &str = "/backups";
/// The page's full path, linked from the emails.
const FULL_PATH: &str = "/admin/backups";
/// FileStore key prefix of every backup.
const PREFIX: &str = "backups/";
const DAY_MS: i64 = 86_400_000;

/// Registers the page, its routes, texts and the Job on the Kernel.
pub(crate) fn register(r: &mut ModuleRegistrar) {
    r.messages(texts::DEFAULTS)
        .back_office_page_in(
            crate::NavGroup::Admin,
            MANAGE,
            PATH,
            "kernel.backup.title",
            90,
            get(page::page),
        )
        .back_office(MANAGE, "/backups/now", post(page::now))
        .back_office(DOWNLOAD, "/backups/export.xlsx", get(export::all))
        .back_office(DOWNLOAD, "/backups/export/{table}", get(export::one))
        .back_office(DOWNLOAD, "/backups/{id}/download", get(download::tar))
        .job(JobSpec::new(KIND), run::run);
}

/// The Job's schedule from `[backup]`, `None` when it is disabled.
pub(crate) fn backup_schedule(
    config: &DeploymentConfig,
) -> core::result::Result<Option<(&'static str, Schedule)>, BuildError> {
    if !config.backup.enabled {
        return Ok(None);
    }
    let s = &config.backup.schedule;
    let parsed = cron::parse(s).map_err(|reason| BuildError::InvalidSchedule {
        kind: KIND.into(),
        schedule: s.clone(),
        reason,
    })?;
    Ok(Some((KIND, parsed)))
}

/// `20261107T190000123Z`: `ms` in UTC, sortable, safe in a FileStore key.
pub(crate) fn stamp(ms: i64) -> String {
    let day = crate::deployment::day::civil_day(ms, 0).replace('-', "");
    let in_day = ms.rem_euclid(DAY_MS);
    let (h, m) = (in_day / 3_600_000, in_day / 60_000 % 60);
    let (s, milli) = (in_day / 1000 % 60, in_day % 1000);
    format!("{day}T{h:02}{m:02}{s:02}{milli:03}Z")
}

/// The key of a run's resume point, next to (not inside) its backup.
fn progress_key(prefix: &str) -> String {
    format!("{}.progress.json", prefix.trim_end_matches('/'))
}

/// The key of a run's saved key hashes `name` (#88), next to its backup.
fn check_key(prefix: &str, name: &str) -> String {
    format!("{}.check/{name}", prefix.trim_end_matches('/'))
}

/// `n` bytes for people: `512 B`, `1.5 MB`.
fn human_bytes(n: i64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    format!("{v:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_and_sizes() {
        // 2026-11-05T00:00:00Z
        assert_eq!(stamp(1_793_836_800_000), "20261105T000000000Z");
        assert_eq!(
            stamp(1_793_836_800_000 + 19 * 3_600_000 + 61_234),
            "20261105T190101234Z"
        );
        assert_eq!(
            progress_key("backups/20261105T000000000Z/"),
            "backups/20261105T000000000Z.progress.json"
        );
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MB");
    }
}
