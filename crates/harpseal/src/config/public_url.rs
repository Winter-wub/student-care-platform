//! `public_url`: the https origin the Deployment is served at (#109), for
//! the URLs a platform fetches without a session (an Announcement image
//! in a LINE Flex hero or a Telegram link preview). A Job has no request
//! to read a host from, so the origin is configured.
//!
//! ```toml
//! public_url = "https://students.example.ac.th"
//! ```
//!
//! The examples also read `HARPSEAL_PUBLIC_URL`, which wins over the TOML.

use serde::{Deserialize, Deserializer};

/// An `https://host[:port]` origin: no path, query or fragment; a trailing
/// `/` is trimmed. Checked with [`crate::render::https_url`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicUrl(String);

impl PublicUrl {
    /// Checks and trims `v`; refuses anything but an https origin.
    pub fn new(v: &str) -> crate::Result<Self> {
        let v = v.trim().trim_end_matches('/');
        let origin_only = v
            .strip_prefix("https://")
            .is_some_and(|rest| !rest.contains(['/', '?', '#', '@']));
        if origin_only && crate::render::https_url(v) {
            Ok(Self(v.to_string()))
        } else {
            Err(crate::Error::Invalid(format!(
                "public_url must be an https origin such as https://students.example.ac.th, not {v:?}"
            )))
        }
    }

    /// The origin, without a trailing `/`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `path` (starting with `/`) on this origin.
    pub fn join(&self, path: &str) -> String {
        format!("{}{path}", self.0)
    }
}

impl<'de> Deserialize<'de> for PublicUrl {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        PublicUrl::new(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::PublicUrl;
    use crate::DeploymentConfig;

    #[test]
    fn only_an_https_origin_is_taken() {
        let u = PublicUrl::new("https://students.seal-bay.invalid/").unwrap();
        assert_eq!(u.as_str(), "https://students.seal-bay.invalid");
        assert_eq!(
            u.join("/announcements/files/x"),
            "https://students.seal-bay.invalid/announcements/files/x"
        );
        assert!(PublicUrl::new("https://seal-bay.invalid:8443").is_ok());
        for bad in [
            "http://seal-bay.invalid",
            "https://seal-bay.invalid/app",
            "https://seal-bay.invalid?x=1",
            "https://user@seal-bay.invalid",
            "seal-bay.invalid",
            "https://",
            "",
        ] {
            assert!(PublicUrl::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_toml_key_is_optional_and_checked() {
        let base = "name = \"X\"\ndefault_locale = \"th\"\n";
        assert!(
            DeploymentConfig::from_toml(base)
                .unwrap()
                .public_url
                .is_none()
        );
        let c = DeploymentConfig::from_toml(&format!(
            "{base}public_url = \"https://students.seal-bay.invalid/\""
        ))
        .unwrap();
        assert_eq!(
            c.public_url.unwrap().as_str(),
            "https://students.seal-bay.invalid"
        );
        let bad = format!("{base}public_url = \"http://students.seal-bay.invalid\"");
        assert!(DeploymentConfig::from_toml(&bad).is_err());
    }
}
