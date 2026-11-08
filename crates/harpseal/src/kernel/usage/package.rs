//! The LINE Official Account package an Administrator picks in the back
//! office, and the monthly Quota that follows it (#115).
//!
//! The effective LINE cap is, in this order:
//!
//! 1. the package or Custom number saved on the usage page (one `setting`
//!    row, key [`KEY`], locale `''`: audited as `setting.update`, carried
//!    by every backup, no migration);
//! 2. `[quotas.line] metered_per_month` of `deployment.toml`, which is the
//!    default and also the Custom number the form starts from, until
//!    someone saves.
//!
//! Every reader of the cap (the usage page, the banner, the send pages'
//! "used X/Y", the watch Job and the tick that enqueues it) goes through
//! [`caps`] at the moment it runs, so a change shows on the next page view
//! or run and a switch in mid-month re-measures the month so far against
//! the new limit. Nothing is cached.
//!
//! The Quota still only warns Staff (GLOSSARY, Quota); it is LINE that
//! refuses Pushes beyond a package that cannot buy more, and the
//! `QuotaExhausted` fallback to email or deferral in `notify` is
//! unchanged.
//!
//! ## Packages
//!
//! Source: LINE for Business Thailand, "LINE Official Account" broadcast
//! message packages, <https://lineforbusiness.com/th/service/line-oa-features/broadcast-message>,
//! read on 2026-11-08. The page lists Free, Basic and Pro (prices exclude
//! 7 % VAT):
//!
//! | Package | Monthly price | Messages included | Extra messages |
//! |---|---|---|---|
//! | Free | none | 300 | cannot be bought |
//! | Basic | 1,280 THB | 15,000 | 0.10 THB each |
//! | Pro | 1,780 THB | 35,000 | 0.06 THB each |
//!
//! Cross-checked with LINE's notices of 2024-06-19 and 2024-07-15
//! (<https://lineforbusiness.com/th/news/20240619_1>,
//! <https://lineforbusiness.com/th/news/20240716_1>): Free is 300 messages
//! and Pro 35,000 at 1,780 THB from 2024-08-01, extra Pro messages 0.06
//! THB. The older help page (500 free messages) is out of date. LINE
//! changes these figures from time to time: update [`PACKAGES`] and the
//! date here and in `docs/guide/quotas.md` in a Core release.

use std::collections::BTreeMap;

use crate::config::DeploymentConfig;
use crate::port_types::{Statement, Value};
use crate::{Result, Services};

/// The Channel kind the package belongs to.
pub(crate) const KIND: &str = "line";
/// The `setting` key holding the choice (`basic`, or `custom:5000`).
pub(crate) const KEY: &str = "kernel.usage.line_package";
/// Where the figures of [`PACKAGES`] were read.
pub(crate) const SOURCE_URL: &str =
    "https://lineforbusiness.com/th/service/line-oa-features/broadcast-message";
/// The day they were read.
pub(crate) const SOURCE_DATE: &str = "2026-11-08";
/// The largest Custom number, so a typo cannot be a limit nobody reaches.
pub(crate) const MAX_CUSTOM: u64 = 100_000_000;

/// One LINE Official Account package in Thailand.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Package {
    /// Stored in the `setting` row.
    pub(crate) id: &'static str,
    /// LINE's own name.
    pub(crate) name: &'static str,
    /// Messages included each month: the limit.
    pub(crate) included: u64,
    /// Whether messages beyond `included` can be bought.
    pub(crate) extra: bool,
}

/// The packages (see the module docs for the source and the date).
pub(crate) const PACKAGES: &[Package] = &[
    Package {
        id: "free",
        name: "Free",
        included: 300,
        extra: false,
    },
    Package {
        id: "basic",
        name: "Basic",
        included: 15_000,
        extra: true,
    },
    Package {
        id: "pro",
        name: "Pro",
        included: 35_000,
        extra: true,
    },
];

/// The package with `id`.
pub(crate) fn package(id: &str) -> Option<&'static Package> {
    PACKAGES.iter().find(|p| p.id == id)
}

/// What the LINE limit comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    /// Nothing saved: `[quotas.line]` (`None`: no Quota).
    Config(Option<u64>),
    /// A package of [`PACKAGES`].
    Package(&'static Package),
    /// A number somebody entered.
    Custom(u64),
}

impl Choice {
    /// The monthly limit, `None` for no Quota.
    pub(crate) fn limit(self) -> Option<u64> {
        match self {
            Choice::Config(limit) => limit,
            Choice::Package(p) => Some(p.included),
            Choice::Custom(n) => Some(n),
        }
    }

    /// The `setting` value that stands for this choice (the audit row's
    /// value before a first save).
    pub(crate) fn encode(self) -> Option<String> {
        match self {
            Choice::Config(limit) => limit.map(|n| format!("custom:{n}")),
            Choice::Package(p) => Some(p.id.to_string()),
            Choice::Custom(n) => Some(format!("custom:{n}")),
        }
    }
}

/// A Custom number: digits only, `1..=`[`MAX_CUSTOM`].
pub(crate) fn custom(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    let digits = !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit());
    let n: u64 = raw.parse().ok().filter(|_| digits)?;
    (1..=MAX_CUSTOM).contains(&n).then_some(n)
}

/// A stored value as a choice, `None` when it is neither a package nor a
/// valid `custom:<n>` (a restored row from another version).
fn decode(stored: &str) -> Option<Choice> {
    match stored.strip_prefix("custom:") {
        Some(n) => custom(n)
            .filter(|n| n.to_string() == stored["custom:".len()..])
            .map(Choice::Custom),
        None => package(stored).map(Choice::Package),
    }
}

/// The choice in force, and the row behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Current {
    /// The saved value, if a row exists.
    pub(crate) stored: Option<String>,
    pub(crate) choice: Choice,
    /// `[quotas.line]`, the default (`None`: no Quota).
    pub(crate) config: Option<u64>,
    /// A row exists but is not a valid choice (an unknown package or a
    /// malformed number): `[quotas.line]` applies and the page asks Staff
    /// to choose again.
    pub(crate) unreadable: bool,
}

impl Current {
    /// What the form was rendered from, to refuse a stale save: SHA-256 of
    /// the row (`-` for none).
    pub(crate) fn version(&self) -> String {
        version(self.stored.as_deref())
    }
}

/// The compare-and-set token of a row value.
pub(crate) fn version(stored: Option<&str>) -> String {
    match stored {
        Some(v) => crate::auth::sha256_hex(v.as_bytes()),
        None => "-".into(),
    }
}

/// The choice in force given the row's value (`None`: no row). Pure; the
/// readers differ only in how they fetched the row.
pub(crate) fn current_of(config: &DeploymentConfig, stored: Option<String>) -> Current {
    let toml = config
        .quotas
        .get(KIND)
        .map(|q| q.metered_per_month)
        .filter(|c| *c > 0);
    let choice = stored.as_deref().and_then(decode);
    let unreadable = stored.is_some() && choice.is_none();
    Current {
        stored,
        choice: choice.unwrap_or(Choice::Config(toml)),
        config: toml,
        unreadable,
    }
}

/// Reads the choice now (one query).
pub(crate) async fn current(s: &Services, config: &DeploymentConfig) -> Result<Current> {
    let rows = s
        .db
        .query(Statement::new("SELECT value FROM setting WHERE key = ? AND locale = ''").bind(KEY))
        .await?;
    let stored = rows
        .get(0, "value")
        .and_then(Value::as_text)
        .map(str::to_string);
    Ok(current_of(config, stored))
}

/// Whether any Quota is in force: `[quotas]` is not empty, or a package is
/// chosen. Reads the `setting` row only when `[quotas]` is empty.
pub(crate) async fn in_force(s: &Services, config: &DeploymentConfig) -> Result<bool> {
    if !config.quotas.is_empty() {
        return Ok(true);
    }
    Ok(current(s, config).await?.choice.limit().is_some())
}

/// The monthly cap of each Channel kind that has one: `[quotas]` with the
/// LINE entry replaced by the saved choice. A cap of 0 still means no
/// Quota (see `quota::cap_of`).
pub(crate) fn caps_with(config: &DeploymentConfig, cur: &Current) -> BTreeMap<String, u64> {
    let mut caps: BTreeMap<String, u64> = config
        .quotas
        .iter()
        .map(|(kind, q)| (kind.clone(), q.metered_per_month))
        .collect();
    if let Some(limit) = cur.choice.limit() {
        caps.insert(KIND.to_string(), limit);
    }
    caps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_have_distinct_ids_and_only_free_cannot_buy_more() {
        for (i, p) in PACKAGES.iter().enumerate() {
            assert_eq!(package(p.id), Some(p));
            assert!(PACKAGES[i + 1..].iter().all(|o| o.id != p.id));
            assert_eq!(p.extra, p.id != "free", "{}", p.id);
        }
        assert_eq!(package("platinum"), None);
    }

    #[test]
    fn custom_is_a_whole_number_in_range() {
        assert_eq!(custom(" 5000 "), Some(5000));
        assert_eq!(custom("1"), Some(1));
        assert_eq!(custom("100000000"), Some(MAX_CUSTOM));
        for bad in [
            "",
            " ",
            "0",
            "-5",
            "+5",
            "1.5",
            "1,500",
            "5 000",
            "๕๐๐๐",
            "100000001",
            "99999999999999999999",
        ] {
            assert_eq!(custom(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn stored_values_round_trip_and_odd_ones_are_refused() {
        for choice in [
            Choice::Package(&PACKAGES[1]),
            Choice::Custom(4200),
            Choice::Custom(1),
        ] {
            let stored = choice.encode().unwrap();
            assert_eq!(decode(&stored), Some(choice), "{stored}");
        }
        for bad in [
            "",
            "custom:",
            "custom:0",
            "custom:007",
            "custom: 5",
            "Basic",
            "free ",
        ] {
            assert_eq!(decode(bad), None, "{bad:?}");
        }
        assert_eq!(Choice::Config(None).encode(), None);
        assert_eq!(
            Choice::Config(Some(300)).encode().as_deref(),
            Some("custom:300")
        );
        assert_eq!(Choice::Package(&PACKAGES[2]).limit(), Some(35_000));
    }
}
