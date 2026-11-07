//! `[backup]`: the nightly backup Job (#78).
//!
//! ```toml
//! [backup]
//! enabled = true            # default true
//! schedule = "0 2 * * *"    # cron, in the Deployment's utc_offset (default 02:00)
//! retention_days = 30       # default 30
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
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            schedule: "0 2 * * *".into(),
            retention_days: 30,
        }
    }
}

impl BackupConfig {
    /// Refuses a schedule outside the cron subset and a retention of 0.
    pub(crate) fn check(&self) -> crate::Result<()> {
        crate::jobs::cron::parse(&self.schedule).map_err(|e| {
            crate::Error::Invalid(format!("[backup] schedule `{}`: {e}", self.schedule))
        })?;
        if self.retention_days == 0 {
            return Err(crate::Error::Invalid(
                "[backup] retention_days must be at least 1".into(),
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
        for bad in [
            "[backup]\nschedule = \"@daily\"",
            "[backup]\nretention_days = 0",
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
