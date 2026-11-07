//! `[announcements]`: settings of the built-in Announcements Module.

use serde::Deserialize;

/// Most announcements the "ข่าวล่าสุด" Menu entry may list: a Card List
/// holds at most 10 Cards (LINE carousel, Telegram's paging alike).
pub const MAX_LATEST: u8 = 10;

/// `[announcements]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct AnnouncementsConfig {
    /// How many of a Student's newest announcements the "ข่าวล่าสุด" Menu
    /// entry lists, `1..=10` (default 5). The last Card keeps its "ดูทั้งหมด"
    /// button to the Mini App list.
    pub latest: u8,
}

impl Default for AnnouncementsConfig {
    fn default() -> Self {
        Self { latest: 5 }
    }
}

impl AnnouncementsConfig {
    /// Refuses `latest` outside `1..=10`.
    pub fn check(&self) -> crate::Result<()> {
        if !(1..=MAX_LATEST).contains(&self.latest) {
            return Err(crate::Error::Invalid(format!(
                "[announcements] latest must be 1..={MAX_LATEST}, not {}",
                self.latest
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_is_checked() {
        assert!(AnnouncementsConfig::default().check().is_ok());
        for bad in [0, 11] {
            assert!(AnnouncementsConfig { latest: bad }.check().is_err());
        }
        assert!(AnnouncementsConfig { latest: 10 }.check().is_ok());
    }
}
