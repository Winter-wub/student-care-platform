//! `[backup]`: the nightly backup Job (#78).
//!
//! ```toml
//! [backup]
//! enabled = true            # default true
//! schedule = "0 2 * * *"    # cron, in the Deployment's utc_offset (default 02:00)
//! retention_days = 30       # default 30
//! max_table_mb = 24         # default 24: largest table the Job dumps
//! max_table_rows = 200000   # default 200,000
//! max_export_cells = 150000 # default 150,000: "Download all data" limit
//! ```

use serde::Deserialize;

/// `[backup]`: when the nightly backup runs and how long backups are kept.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct BackupConfig {
    /// Run the scheduled backup Job. Default `true`. `false` stops the
    /// schedule only: the Admin page still lists and downloads backups and
    /// still offers "back up now".
    pub enabled: bool,
    /// When it runs: a cron expression in the subset
    /// [`crate::JobSpec::schedule`] accepts, in the Deployment's
    /// `utc_offset`. Default `0 2 * * *` (02:00 every night).
    pub schedule: String,
    /// Backups older than this many days are deleted after the next
    /// successful backup (never the newest successful one). Default 30,
    /// at least 1.
    pub retention_days: u32,
    /// The largest table the Job dumps, in MB of NDJSON (estimated before
    /// reading it). A table is one entry built in memory (format 1 has no
    /// table parts yet), and a Worker has 128 MB, with the entry held about
    /// three times over while it is written to R2: default 24. A larger
    /// table fails the run, naming it; back such a Deployment up natively
    /// (`harpseal backup`), or raise this on a native server.
    pub max_table_mb: u32,
    /// The most rows of one table the Job dumps: default 200,000, that is
    /// 400 D1 queries of 500 rows, inside a Worker invocation's 1,000
    /// subrequests. Larger fails the run like `max_table_mb`.
    pub max_table_rows: u64,
    /// The most cells (rows × exported columns, all tables) "Download all
    /// data" builds; above it the page refuses before building anything.
    /// The workbook is built in memory at about 200 bytes per cell
    /// (measured natively: 100,000 rows × 10 columns peaked at 218 MB):
    /// default 150,000 keeps it near 30 MB on a 128 MB Worker. Raise it on
    /// a native server.
    pub max_export_cells: u64,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            schedule: "0 2 * * *".into(),
            retention_days: 30,
            max_table_mb: 24,
            max_table_rows: 200_000,
            max_export_cells: 150_000,
        }
    }
}

impl BackupConfig {
    /// Refuses a schedule outside the cron subset and a retention of 0.
    pub(crate) fn check(&self) -> crate::Result<()> {
        crate::jobs::cron::parse(&self.schedule).map_err(|e| {
            crate::Error::Invalid(format!("[backup] schedule `{}`: {e}", self.schedule))
        })?;
        if self.retention_days == 0 || self.max_table_mb == 0 || self.max_table_rows == 0 {
            return Err(crate::Error::Invalid(
                "[backup] retention_days, max_table_mb and max_table_rows must be at least 1"
                    .into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::DeploymentConfig;

    const BASE: &str = "name = \"X\"\ndefault_locale = \"th\"\n";

    #[test]
    fn defaults_and_refusals() {
        let c = DeploymentConfig::from_toml(BASE).unwrap();
        assert!(c.backup.enabled);
        assert_eq!(c.backup.schedule, "0 2 * * *");
        assert_eq!(c.backup.retention_days, 30);
        assert_eq!(
            (c.backup.max_table_mb, c.backup.max_table_rows),
            (24, 200_000)
        );
        for bad in [
            "[backup]\nschedule = \"@daily\"",
            "[backup]\nretention_days = 0",
            "[backup]\nmax_table_mb = 0",
            "[backup]\nkeep_days = 3",
        ] {
            assert!(
                DeploymentConfig::from_toml(&format!("{BASE}{bad}")).is_err(),
                "accepted: {bad}"
            );
        }
        let c = DeploymentConfig::from_toml(&format!(
            "{BASE}[backup]\nenabled = false\nschedule = \"30 3 * * *\"\nretention_days = 7"
        ))
        .unwrap();
        assert!(!c.backup.enabled);
        assert_eq!(c.backup.retention_days, 7);
    }
}
