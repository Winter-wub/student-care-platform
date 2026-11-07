//! The kinds of a Module setting (#75) and their server-side checks.

/// Longest single-line text or URL, in characters.
pub const MAX_LINE_CHARS: usize = 500;
/// Longest multi-line text, in characters.
pub const MAX_MULTILINE_CHARS: usize = 5000;

/// The type of a Module setting, checked on the server on every save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingKind {
    /// One line, 1..=500 characters, no control characters.
    Text,
    /// Several lines (CRLF stored as LF), 1..=5000 characters.
    MultilineText,
    /// A whole number (`i64`), e.g. `-3` or `1500`; no separators.
    Number,
    /// A calendar date `YYYY-MM-DD`.
    Date,
    /// An `https://` URL with a host, up to 500 characters.
    Url,
}

impl SettingKind {
    /// The value as stored (trimmed; LF line breaks), or the catalog key
    /// of why it is refused.
    pub fn check(self, raw: &str) -> core::result::Result<String, &'static str> {
        let v = match self {
            SettingKind::MultilineText => raw.replace("\r\n", "\n").trim().to_string(),
            _ => raw.trim().to_string(),
        };
        if v.is_empty() {
            return Err("kernel.admin.module_settings.err_empty");
        }
        let max = match self {
            SettingKind::MultilineText => MAX_MULTILINE_CHARS,
            _ => MAX_LINE_CHARS,
        };
        if v.chars().count() > max {
            return Err("kernel.admin.module_settings.err_long");
        }
        let allowed =
            |c: char| !c.is_control() || (self == SettingKind::MultilineText && c == '\n');
        if !v.chars().all(allowed) {
            return Err("kernel.admin.module_settings.err_text");
        }
        match self {
            SettingKind::Text | SettingKind::MultilineText => Ok(v),
            SettingKind::Number => number(&v)
                .map(|n| n.to_string())
                .ok_or("kernel.admin.module_settings.err_number"),
            SettingKind::Date if date(&v) => Ok(v),
            SettingKind::Date => Err("kernel.admin.module_settings.err_date"),
            SettingKind::Url if https_url(&v) => Ok(v),
            SettingKind::Url => Err("kernel.admin.module_settings.err_url"),
        }
    }

    /// The catalog key naming the kind on the settings page.
    pub(crate) fn label_key(self) -> &'static str {
        match self {
            SettingKind::Text => "kernel.admin.module_settings.kind_text",
            SettingKind::MultilineText => "kernel.admin.module_settings.kind_multiline",
            SettingKind::Number => "kernel.admin.module_settings.kind_number",
            SettingKind::Date => "kernel.admin.module_settings.kind_date",
            SettingKind::Url => "kernel.admin.module_settings.kind_url",
        }
    }
}

pub(super) fn number(v: &str) -> Option<i64> {
    let digits = v.strip_prefix('-').unwrap_or(v);
    let plain = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    plain.then(|| v.parse().ok()).flatten()
}

/// `YYYY-MM-DD`, a real calendar day.
fn date(v: &str) -> bool {
    let b = v.as_bytes();
    let shape = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !shape {
        return false;
    }
    let (y, m, d): (u32, u32, u32) = match (v[..4].parse(), v[5..7].parse(), v[8..].parse()) {
        (Ok(y), Ok(m), Ok(d)) => (y, m, d),
        _ => return false,
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    y >= 1 && (1..=days).contains(&d)
}

/// `https://host[:port][/…]`: a host of letters, digits, `.` and `-`, no
/// user info, no spaces.
fn https_url(v: &str) -> bool {
    let Some(rest) = v.strip_prefix("https://") else {
        return false;
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    let host_ok = !host.is_empty()
        && !host.starts_with(['.', '-'])
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let port_ok = port.is_none_or(|p| p.parse::<u16>().is_ok_and(|n| n > 0));
    host_ok && port_ok && !tail.chars().any(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::SettingKind::*;

    #[test]
    fn each_kind_checks_its_values() {
        assert_eq!(Text.check("  ธนาคาร ทะเลใส "), Ok("ธนาคาร ทะเลใส".into()));
        assert!(Text.check("a\nb").is_err() && Text.check("   ").is_err());
        assert_eq!(MultilineText.check("a\r\nb\n"), Ok("a\nb".into()));
        assert!(MultilineText.check(&"x".repeat(5001)).is_err());
        assert_eq!(Number.check(" -1500 "), Ok("-1500".into()));
        for bad in ["1,500", "1.5", "", "-", "+1", "99999999999999999999"] {
            assert!(Number.check(bad).is_err(), "{bad}");
        }
        assert!(Date.check("2028-02-29").is_ok());
        for bad in ["2027-02-29", "2027-13-01", "2027-1-01", "01/02/2027"] {
            assert!(Date.check(bad).is_err(), "{bad}");
        }
        assert!(
            Url.check("https://pay.seal-bay.invalid/fees?term=2")
                .is_ok()
        );
        assert!(Url.check("https://pay.seal-bay.invalid:8443").is_ok());
        for bad in [
            "http://pay.seal-bay.invalid",
            "javascript:alert(1)",
            "https://",
            "https://a b",
            "https://user@host",
            "https://host:0",
        ] {
            assert!(Url.check(bad).is_err(), "{bad}");
        }
    }
}
