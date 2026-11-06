//! The edit form's fields: reading them back and checking them against
//! the `[roster.attributes]` schema.

use std::collections::BTreeMap;

use super::StudentFields;
use crate::config::{AttributeSpec, RosterConfig};
use crate::message::Locale;

/// A submitted form: field name → value (the last one wins).
pub(super) type FormMap = BTreeMap<String, String>;

/// Form field of attribute `key`.
pub(super) fn attr_input(key: &str) -> String {
    format!("attr.{key}")
}

/// Errors per form field: catalog keys of the messages.
pub(super) type FormErrors = BTreeMap<String, &'static str>;

/// Longest display name, in characters.
const MAX_NAME_CHARS: usize = 200;

/// The stored fields as the edit form shows them.
pub(super) fn rendered(roster: &RosterConfig, f: &StudentFields) -> FormMap {
    let mut m = FormMap::new();
    m.insert("display_name".into(), f.display_name.clone());
    m.insert("email".into(), f.email.clone().unwrap_or_default());
    m.insert("locale".into(), f.locale.0.clone());
    for key in roster.attributes.keys() {
        let value = AttributeSpec::display(f.attributes.get(key));
        m.insert(attr_input(key), value);
    }
    m
}

/// Reads the edit form over `current`, which it was rendered from. A field
/// whose submitted text is what the form showed (or that was not
/// submitted) keeps its stored value untouched and unchecked, so values
/// that do not round-trip through text (`3.0`, surrounding spaces, an
/// enum value no longer listed) never count as edits. Every edited field
/// is checked (core fields, then the schema attributes; attributes outside
/// the schema are kept), and every problem is reported, not just the
/// first.
pub(super) fn parse_edit(
    roster: &RosterConfig,
    current: &StudentFields,
    form: &FormMap,
) -> Result<StudentFields, FormErrors> {
    let shown = rendered(roster, current);
    let edited = |k: &str| form.get(k).is_some_and(|v| shown.get(k) != Some(v));
    let get = |k: &str| form.get(k).map(|v| v.trim()).unwrap_or_default();
    let mut errors = FormErrors::new();
    let mut out = current.clone();
    if edited("display_name") {
        let name = get("display_name");
        if name.is_empty() {
            errors.insert("display_name".into(), "kernel.students.err_required");
        } else if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
            errors.insert("display_name".into(), "kernel.students.err_too_long");
        }
        out.display_name = name.to_string();
    }
    if edited("email") {
        let email = get("email");
        if !email.is_empty() && !is_email(email) {
            errors.insert("email".into(), "kernel.students.err_email");
        }
        out.email = (!email.is_empty()).then(|| email.to_string());
    }
    if edited("locale") {
        let locale = get("locale");
        if !is_locale(locale) {
            errors.insert("locale".into(), "kernel.students.err_locale");
        }
        out.locale = Locale(locale.to_string());
    }
    for (key, spec) in &roster.attributes {
        let input = attr_input(key);
        if !edited(&input) {
            continue;
        }
        match spec.parse(get(&input)) {
            Ok(Some(v)) => {
                out.attributes.insert(key.clone(), v);
            }
            Ok(None) => {
                out.attributes.remove(key);
            }
            Err(e) => {
                errors.insert(input, e.key());
            }
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// A plausible address: one `@`, a dot in the domain, no spaces, at most
/// 254 bytes. The roster import (#24) applies its own syntax check.
pub(super) fn is_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    s.len() <= 254
        && !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// A language tag: 2 to 10 ASCII letters, digits or `-`, starting with a
/// letter.
pub(super) fn is_locale(s: &str) -> bool {
    (2..=10).contains(&s.len())
        && s.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Percent-encodes everything but ASCII letters, digits and `-._~`.
pub(super) fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_and_locales() {
        assert!(is_email("nok@student.sealbay.example"));
        for bad in ["nok", "@x.y", "a@b", "a b@c.d", "a@b@c.d", "a@.b", "a@b."] {
            assert!(!is_email(bad), "{bad}");
        }
        assert!(is_locale("th") && is_locale("en-GB"));
        assert!(!is_locale("t") && !is_locale("1x") && !is_locale("th_TH"));
        assert_eq!(urlencode("นก a"), "%E0%B8%99%E0%B8%81%20a");
    }
}
