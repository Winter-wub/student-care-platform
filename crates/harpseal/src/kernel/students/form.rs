//! The edit form's fields: reading them back and checking them against
//! the `[roster.attributes]` schema.

use std::collections::BTreeMap;

use super::StudentFields;
use super::email::is_email;
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
/// first. An attribute required only for some Students (`optional_when`,
/// `required_when`) is checked when it or the attribute it depends on
/// changed.
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
    // `optional_when` / `required_when`: checked when the field or the
    // one it depends on changed, against the values after the edit.
    for (key, spec) in &roster.attributes {
        let Some((other, _)) = spec.condition() else {
            continue;
        };
        let input = attr_input(key);
        let touched = edited(&input) || edited(&attr_input(other));
        let failed = errors.contains_key(&input) || errors.contains_key(&attr_input(other));
        if touched
            && !failed
            && !out.attributes.contains_key(key)
            && spec.required_for(&out.attributes)
        {
            errors.insert(input, "kernel.students.err_required_here");
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
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
        assert!(is_locale("th") && is_locale("en-GB"));
        assert!(!is_locale("t") && !is_locale("1x") && !is_locale("th_TH"));
        assert_eq!(urlencode("นก a"), "%E0%B8%99%E0%B8%81%20a");
    }

    #[test]
    fn a_conditional_attribute_is_checked_when_it_or_its_condition_changes() {
        let c = crate::DeploymentConfig::from_toml(
            "name = \"X\"\ndefault_locale = \"th\"\n\
             [roster.attributes.status]\ncolumn = \"s\"\ntype = \"enum\"\nvalues = [\"current\", \"graduated\"]\n\
             [roster.attributes.year]\ncolumn = \"y\"\ntype = \"integer\"\noptional_when = { status = [\"graduated\"] }\n",
        )
        .unwrap();
        let mut current = StudentFields::new("นก", None, Locale::th());
        current.attributes.insert("status".into(), "current".into());
        current.attributes.insert("year".into(), 3.into());
        let mut form = rendered(&c.roster, &current);
        // Clearing the year of a current Student is refused...
        form.insert("attr.year".into(), String::new());
        let errors = parse_edit(&c.roster, &current, &form).unwrap_err();
        assert_eq!(errors["attr.year"], "kernel.students.err_required_here");
        // ...and allowed once the Student is a graduate.
        form.insert("attr.status".into(), "graduated".into());
        let out = parse_edit(&c.roster, &current, &form).unwrap();
        assert!(!out.attributes.contains_key("year"));
        // A graduate without a year who becomes current needs one.
        let mut graduate = StudentFields::new("ปลา", None, Locale::th());
        graduate
            .attributes
            .insert("status".into(), "graduated".into());
        let mut form = rendered(&c.roster, &graduate);
        form.insert("attr.status".into(), "current".into());
        let errors = parse_edit(&c.roster, &graduate, &form).unwrap_err();
        assert!(errors.contains_key("attr.year"));
        // An edit of another field checks nothing else.
        let mut form = rendered(&c.roster, &graduate);
        form.insert("display_name".into(), "ปลาทอง".into());
        let mut legacy = graduate.clone();
        legacy.attributes.insert("status".into(), "current".into());
        let form_legacy = {
            let mut f = rendered(&c.roster, &legacy);
            f.insert("display_name".into(), "ปลาทอง".into());
            f
        };
        assert!(parse_edit(&c.roster, &legacy, &form_legacy).is_ok());
        assert!(parse_edit(&c.roster, &graduate, &form).is_ok());
    }
}
