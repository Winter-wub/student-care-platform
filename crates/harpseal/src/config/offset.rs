//! `utc_offset`: a fixed offset such as `+07:00`, parsed to minutes.

/// `+07:00` / `-03:30` -> minutes.
pub(super) fn parse_offset(s: &str) -> Option<i32> {
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
    use super::parse_offset;
    use crate::DeploymentConfig;

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
