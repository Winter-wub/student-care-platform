//! `[mini_app]`: how long an App Session lives without use (#86).
//!
//! ```toml
//! [mini_app]
//! idle_minutes = 15
//! ```

use serde::Deserialize;

/// `[mini_app]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct MiniAppConfig {
    /// An App Session with no request for this long ends; each request
    /// moves the end forward (at most once a minute), never past one hour
    /// from the exchange. Default 15, allowed 1..=60. Shorter protects a
    /// shared computer better (Back, a reload, a reopened tab can bring a
    /// page back without a new launch); longer re-asks the platform less.
    pub idle_minutes: u32,
}

impl Default for MiniAppConfig {
    fn default() -> Self {
        Self { idle_minutes: 15 }
    }
}

impl MiniAppConfig {
    /// `idle_minutes` in milliseconds.
    pub(crate) fn idle_ms(&self) -> i64 {
        i64::from(self.idle_minutes) * 60_000
    }

    pub(crate) fn check(&self) -> crate::Result<()> {
        if (1..=60).contains(&self.idle_minutes) {
            Ok(())
        } else {
            Err(crate::Error::Invalid(
                "[mini_app] idle_minutes must be 1..=60".into(),
            ))
        }
    }
}
