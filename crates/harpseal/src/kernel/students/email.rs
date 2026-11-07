//! The one email check of the Core (#71): the roster import, the Student
//! edit form, the Students list's missing-data badge and the chat flow's
//! claim all ask the same question, so a Student saved by Staff is never
//! flagged invalid by the list, and the claim never sends a code to an
//! address the roster would have warned about.

/// A syntactically plausible address: one `@`, a non-empty local part of
/// printable ASCII without spaces (ASCII only: an address with Thai or
/// other non-ASCII characters, allowed by RFC 6531, is refused, because
/// the Link's email OTP goes through providers that may not accept it), a
/// domain of dot-separated labels (letters, digits, `-`, not at a label's
/// ends) with a dot; at most 254 bytes.
pub(crate) fn is_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    let local_ok = !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .bytes()
            .all(|b| b.is_ascii_graphic() && !b"@\"(),:;<>[\\]".contains(&b));
    let label_ok = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    s.len() <= 254 && local_ok && domain.contains('.') && domain.split('.').all(label_ok)
}

/// The key two addresses are compared by: trimmed, lowercase.
pub(crate) fn key(email: &str) -> String {
    email.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_check_for_staff_and_the_roster() {
        assert!(is_email("first.last+tag@student.sealbay.example"));
        for bad in [
            "nok",
            "@x.y",
            "a@b",
            "a b@c.d",
            "a@b@c.d",
            "a@.b",
            "a@b.",
            "นก@x.ac.th",
            "a@x..example",
            "a@-x.example",
        ] {
            assert!(!is_email(bad), "{bad}");
        }
        assert_eq!(
            key("  Nok@Student.SealBay.example "),
            "nok@student.sealbay.example"
        );
    }
}
