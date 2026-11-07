//! No Locale in `deployment.toml` may be empty (#75): rows of the
//! `setting` table with Locale `''` are the Consent version and Module
//! settings, never texts, so the Texts page must never offer that Locale.

use super::DeploymentConfig;

/// Refuses an empty `default_locale` and a `[texts.""]` table.
pub(super) fn check(c: &DeploymentConfig) -> crate::Result<()> {
    let mut all = std::iter::once(&c.default_locale.0).chain(c.texts.keys());
    if all.any(|l| l.trim().is_empty()) {
        return Err(crate::Error::Invalid(
            "a Locale in deployment.toml is empty (default_locale or a [texts] table)".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::DeploymentConfig;

    #[test]
    fn an_empty_locale_is_refused() {
        let base = "name = \"X\"\n";
        for bad in [
            "default_locale = \"\"",
            "default_locale = \"th\"\n[texts.\"\"]\n\"fees.title\" = \"x\"",
            "default_locale = \"th\"\n[texts.\" \"]\n\"fees.title\" = \"x\"",
        ] {
            assert!(
                DeploymentConfig::from_toml(&format!("{base}{bad}")).is_err(),
                "{bad}"
            );
        }
        assert!(DeploymentConfig::from_toml("name = \"X\"\ndefault_locale = \"th\"").is_ok());
    }
}
