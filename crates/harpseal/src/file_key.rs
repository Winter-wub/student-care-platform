//! [`check_file_key`]: the one rule for `FileStore` keys, shared by every
//! runtime so a key that works on Workers (R2) also works natively (a
//! directory, S3).

use crate::{Error, Result};

/// Accepts a key made of `/`-separated segments. Refused: an empty key, a
/// leading `/`, empty segments (`a//b`, a trailing `/`), `.` and `..`
/// segments, `\`, `:` and control characters. Keys are opaque otherwise
/// (UTF-8 is fine).
pub fn check_file_key(key: &str) -> Result<()> {
    let bad = key.is_empty()
        || key.contains(['\\', ':'])
        || key.chars().any(char::is_control)
        || key
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == "..");
    if bad {
        return Err(Error::Invalid(format!("bad file key {key:?}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_file_key;

    #[test]
    fn accepts_plain_and_nested_keys() {
        for k in [
            "a",
            "roster/2026/import.csv",
            "ใบ/แจ้ง.pdf",
            "x.tmp",
            ".hidden",
        ] {
            assert!(check_file_key(k).is_ok(), "{k:?}");
        }
    }

    #[test]
    fn refuses_unsafe_keys() {
        for k in [
            "",
            "/etc/passwd",
            "a/",
            "a//b",
            ".",
            "..",
            "../x",
            "a/./b",
            "a/../b",
            "a\\b",
            "c:/x",
            "a:b",
            "a\nb",
            "a\0b",
        ] {
            assert!(check_file_key(k).is_err(), "{k:?} accepted");
        }
    }
}
