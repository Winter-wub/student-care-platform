//! `[mini_app]`: how long an App Session lives without use, and how many
//! platform tokens one IP group may have verified a minute (#86).
//!
//! ```toml
//! [mini_app]
//! idle_minutes = 15
//! verify_per_minute = 300
//! ```

use serde::Deserialize;

/// `[mini_app]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct MiniAppConfig {
    /// An App Session with no request for this long ends; each request
    /// moves the end forward (at most once a minute), never past one hour
    /// from the exchange. Default 15, allowed 2..=60 (with 1 the once-a-
    /// minute step would never move it). Shorter protects a shared
    /// computer better (Back, a reload, a reopened tab can bring a page
    /// back without a new launch); longer re-asks the platform less.
    pub idle_minutes: u32,
    /// Exchanges per IP group (IPv4 address, IPv6 /64) per minute on a
    /// Channel whose verification is a network call (LIFF), counted before
    /// the call, accepted or not. Default 300, allowed 30..=100000: high
    /// enough for a class behind one campus NAT, low enough that nobody
    /// can make LINE throttle the Deployment. Refused tokens have their
    /// own, fixed budget of 30 a minute.
    pub verify_per_minute: u32,
}

impl Default for MiniAppConfig {
    fn default() -> Self {
        Self {
            idle_minutes: 15,
            verify_per_minute: 300,
        }
    }
}

impl MiniAppConfig {
    /// `idle_minutes` in milliseconds.
    pub(crate) fn idle_ms(&self) -> i64 {
        i64::from(self.idle_minutes) * 60_000
    }

    pub(crate) fn check(&self) -> crate::Result<()> {
        if !(2..=60).contains(&self.idle_minutes) {
            return Err(crate::Error::Invalid(
                "[mini_app] idle_minutes must be 2..=60".into(),
            ));
        }
        if !(30..=100_000).contains(&self.verify_per_minute) {
            return Err(crate::Error::Invalid(
                "[mini_app] verify_per_minute must be 30..=100000".into(),
            ));
        }
        Ok(())
    }
}
