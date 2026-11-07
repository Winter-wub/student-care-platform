//! `deployment.toml`: data-only configuration beside the binary. Secrets
//! never live here; they come from the environment or Worker secrets.

use std::collections::BTreeMap;

use serde::Deserialize;

mod announcements;
mod backup;
mod roster;

pub use announcements::AnnouncementsConfig;
pub use backup::BackupConfig;
pub use roster::{AttrError, AttributeSpec, AttributeType, LocaleColumn, RosterConfig, TagsColumn};

use crate::manifest::Catalog;
use crate::message::Locale;

/// The whole `deployment.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DeploymentConfig {
    /// The customer's name, shown in page chrome.
    pub name: String,
    /// Locale content falls back to.
    pub default_locale: Locale,
    /// Fixed offset of the Deployment's time zone from UTC, `+07:00`.
    /// Display and day buckets (`usage_daily`) use it.
    #[serde(default = "default_offset")]
    pub utc_offset: String,
    /// Look of the back office and Mini App.
    #[serde(default)]
    pub branding: BrandingConfig,
    /// Initial Roles: Role name -> Permission ids. `build()` rejects an id
    /// no Module declared. Merged into the database on every migrate,
    /// add-only: removing a Role or a Permission here does not remove it
    /// from the database (do that in the back office).
    #[serde(default)]
    pub roles: BTreeMap<String, Vec<String>>,
    /// Per-Channel Quotas, keyed by Channel kind (`[quotas.line]`).
    #[serde(default)]
    pub quotas: BTreeMap<String, QuotaConfig>,
    /// Locale -> key -> text; overrides Module defaults.
    #[serde(default)]
    pub texts: BTreeMap<String, BTreeMap<String, String>>,
    /// Email-OTP parameters.
    #[serde(default)]
    pub otp: OtpConfig,
    /// How the chat flow reads a student id and whether Students may unlink.
    #[serde(default)]
    pub student: StudentConfig,
    /// The privacy notice version the Consent Card asks for.
    #[serde(default)]
    pub consent: ConsentConfig,
    /// What happens to free text and files after Link.
    #[serde(default)]
    pub inbound: InboundConfig,
    /// How long pruned rows are kept.
    #[serde(default)]
    pub retention: RetentionConfig,
    /// Staff sign-in settings.
    #[serde(default)]
    pub staff: StaffConfig,
    /// Roster columns and the `student.attributes` schema
    /// ([`RosterConfig`]).
    #[serde(default)]
    pub roster: RosterConfig,
    /// The nightly backup ([`BackupConfig`], #78).
    #[serde(default)]
    pub backup: BackupConfig,
    /// The Announcements Module's settings ([`AnnouncementsConfig`]).
    #[serde(default)]
    pub announcements: AnnouncementsConfig,
}

fn default_offset() -> String {
    "+00:00".into()
}

/// `[branding]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct BrandingConfig {
    /// Name in the bar and page titles; the Deployment `name` otherwise.
    pub name: Option<String>,
    /// Main colour, CSS hex (`#rgb` or `#rrggbb`).
    pub primary_color: String,
    /// Logo URL or path, if any (shown in the bar).
    pub logo: Option<String>,
    /// Optional web font stylesheet URL (`https://` or a `/` path); the
    /// system Thai font otherwise. The CSS must name the font itself.
    pub font_url: Option<String>,
}

/// Brand colour when `[branding]` gives none.
pub(crate) const DEFAULT_PRIMARY: &str = "#0b5cad";

impl Default for BrandingConfig {
    fn default() -> Self {
        Self {
            name: None,
            primary_color: DEFAULT_PRIMARY.into(),
            logo: None,
            font_url: None,
        }
    }
}

impl BrandingConfig {
    /// Refuses a colour that is not CSS hex and URLs that are not
    /// `https://` or a `/` path (they go into the page head).
    pub(crate) fn check(&self) -> crate::Result<()> {
        let hex = self.primary_color.strip_prefix('#').unwrap_or("x");
        let ok = matches!(hex.len(), 3 | 6) && hex.bytes().all(|b| b.is_ascii_hexdigit());
        if !ok {
            let c = &self.primary_color;
            return Err(crate::Error::Invalid(format!(
                "[branding] primary_color `{c}`"
            )));
        }
        for (key, url) in [("logo", &self.logo), ("font_url", &self.font_url)] {
            let url = url.as_deref().unwrap_or("/");
            let ok =
                url.starts_with("https://") || (url.starts_with('/') && !url.starts_with("//"));
            if !ok {
                return Err(crate::Error::Invalid(format!(
                    "[branding] {key} must start with https:// or /"
                )));
            }
        }
        Ok(())
    }
}

/// `[staff]`: Staff sign-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct StaffConfig {
    /// Mark the session cookies `Secure` (sent over HTTPS only). Default
    /// `true`. Set `false` only for local development over plain
    /// `http://` from another device (browsers already accept `Secure`
    /// cookies on `http://localhost`); never in production.
    pub cookie_secure: bool,
    /// Take the client address from `X-Forwarded-For` (right-most public
    /// hop) and `CF-Connecting-IP`. Default `false`. Turn it on only when a
    /// native server is reachable solely through a proxy you run, which
    /// appends the address it saw; otherwise clients could forge the
    /// address in audit rows, sessions and Security Events. Workers always
    /// use `CF-Connecting-IP`.
    pub trust_forwarded_for: bool,
}

impl Default for StaffConfig {
    fn default() -> Self {
        Self {
            cookie_secure: true,
            trust_forwarded_for: false,
        }
    }
}

/// `[quotas.<channel>]`: warns Staff, never stops sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct QuotaConfig {
    /// Metered pushes allowed per calendar month.
    pub metered_per_month: u64,
}

/// `[otp]`, defaults from the spec.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct OtpConfig {
    /// Digits in a code.
    pub digits: u8,
    /// Minutes a code stays valid.
    pub ttl_minutes: u32,
    /// Wrong answers before the lock.
    pub max_attempts: u8,
    /// Codes per hour per student code.
    pub resends_per_hour: u8,
    /// Minutes a Student stays locked.
    pub lock_minutes: u32,
}

impl Default for OtpConfig {
    fn default() -> Self {
        Self {
            digits: 6,
            ttl_minutes: 5,
            max_attempts: 3,
            resends_per_hour: 3,
            lock_minutes: 30,
        }
    }
}

/// `[student]`: the chat flow's student-id step and self-unlink.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct StudentConfig {
    /// Shape of a student id, regex-free: `#` is an ASCII digit, `@` an
    /// ASCII letter, `?` either, any other character must match itself;
    /// `|` separates alternatives (`"########|@#######"`). `None` (the
    /// default): 4 to 20 letters, digits or `-`. Spaces typed by the
    /// Student are removed before the check.
    pub id_pattern: Option<String>,
    /// Whether the Menu offers "unlink" to the Student (spec: default on).
    pub self_unlink: bool,
}

impl Default for StudentConfig {
    fn default() -> Self {
        Self {
            id_pattern: None,
            self_unlink: true,
        }
    }
}

/// `[consent]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct ConsentConfig {
    /// The current privacy notice version. A `setting` row
    /// `kernel.consent.version` (Locale `''`) overrides it, so Staff can
    /// bump it without a deploy. Any change asks linked Students again.
    pub version: String,
}

impl Default for ConsentConfig {
    fn default() -> Self {
        Self {
            version: "1".into(),
        }
    }
}

/// `[inbound]`: free text and files a linked Student sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct InboundConfig {
    /// Minutes between two fallback answers to one account (spec: 60).
    pub fallback_cooldown_minutes: u32,
    /// Largest file copied into the FileStore, in MB (spec: 20).
    pub max_file_mb: u32,
}

impl Default for InboundConfig {
    fn default() -> Self {
        Self {
            fallback_cooldown_minutes: 60,
            max_file_mb: 20,
        }
    }
}

/// `[retention]`, in days.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct RetentionConfig {
    /// `outbound_message` rows.
    pub outbound_message_days: u32,
    /// `done` Job rows (spec: 30). `failed` rows are kept 90 days, or
    /// `job_days` if longer.
    pub job_days: u32,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            outbound_message_days: 90,
            job_days: 30,
        }
    }
}

impl DeploymentConfig {
    /// Parses and checks `deployment.toml`.
    pub fn from_toml(src: &str) -> crate::Result<Self> {
        let c: Self = toml::from_str(src).map_err(|e| crate::Error::Invalid(e.to_string()))?;
        c.utc_offset_minutes()?;
        if !(4..=9).contains(&c.otp.digits) || c.otp.max_attempts == 0 {
            return Err(crate::Error::Invalid(
                "[otp] digits must be 4..=9 and max_attempts at least 1".into(),
            ));
        }
        if c.consent.version.trim().is_empty() {
            return Err(crate::Error::Invalid("[consent] version is empty".into()));
        }
        c.branding.check()?;
        c.roster.check()?;
        c.backup.check()?;
        c.announcements.check()?;
        Ok(c)
    }

    /// `utc_offset` in minutes east of UTC.
    pub fn utc_offset_minutes(&self) -> crate::Result<i32> {
        parse_offset(&self.utc_offset)
            .ok_or_else(|| crate::Error::Invalid(format!("utc_offset `{}`", self.utc_offset)))
    }

    /// Applies `texts` on top of a catalog already holding Module defaults.
    pub(crate) fn apply_texts(&self, catalog: &mut Catalog) {
        catalog.default_locale = Some(self.default_locale.clone());
        for (locale, entries) in &self.texts {
            for (k, v) in entries {
                catalog.insert(locale, k, v);
            }
        }
    }
}

/// `+07:00` / `-03:30` -> minutes.
fn parse_offset(s: &str) -> Option<i32> {
    let (sign, rest) = match s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => return None,
    };
    let (h, m) = rest.split_once(':')?;
    let two_digits = |p: &str| p.len() == 2 && p.bytes().all(|b| b.is_ascii_digit());
    if !two_digits(h) || !two_digits(m) {
        return None;
    }
    let (h, m): (i32, i32) = (h.parse().ok()?, m.parse().ok()?);
    (h <= 14 && m < 60).then_some(sign * (h * 60 + m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_keys_are_refused_at_every_level() {
        let base = "name = \"X\"\ndefault_locale = \"th\"\n";
        for extra in [
            "nmae = \"typo\"",
            "[branding]\nprimary_colour = \"#000\"",
            "[otp]\ndigit = 6",
            "[student]\npattern = \"#\"",
            "[consent]\nversoin = \"2\"",
            "[inbound]\nmax_file = 1",
            "[otp]\ndigits = 12",
            "[retention]\njobs_days = 1",
            "[quotas.line]\nmetered_per_month = 1\nper_day = 2",
            "[staff]\ncookie_secur = false",
            "[branding]\nprimary_color = \"red;}body{x\"",
            "[branding]\nfont_url = \"javascript:alert(1)\"",
            "[branding]\nlogo = \"//evil.example/x.png\"",
        ] {
            let err = DeploymentConfig::from_toml(&format!("{base}{extra}"));
            assert!(err.is_err(), "accepted: {extra}");
        }
    }

    #[test]
    fn minimal_toml_gets_spec_defaults() {
        let c = DeploymentConfig::from_toml("name = \"X\"\ndefault_locale = \"th\"").unwrap();
        assert_eq!(c.otp, OtpConfig::default());
        assert_eq!(c.retention.outbound_message_days, 90);
        assert!(c.student.self_unlink && c.student.id_pattern.is_none());
        assert_eq!(c.consent.version, "1");
        assert_eq!(c.inbound, InboundConfig::default());
        assert_eq!(c.utc_offset_minutes().unwrap(), 0);
        assert!(c.staff.cookie_secure);
        assert_eq!(c.branding.primary_color, DEFAULT_PRIMARY);
    }

    #[test]
    fn offsets() {
        assert_eq!(parse_offset("+07:00"), Some(420));
        assert_eq!(parse_offset("-03:30"), Some(-210));
        assert_eq!(parse_offset("07:00"), None);
        assert_eq!(parse_offset("+7:00"), None);
        assert_eq!(parse_offset("++7:00"), None);
        assert_eq!(parse_offset("+-7:00"), None);
        assert_eq!(parse_offset("-+07:00"), None);
        assert_eq!(parse_offset("+15:00"), None);
        let bad = "name = \"X\"\ndefault_locale = \"th\"\nutc_offset = \"Asia/Bangkok\"";
        assert!(DeploymentConfig::from_toml(bad).is_err());
    }
}
