//! `[roster]`: how a roster file maps onto Students, and the schema of
//! `student.attributes` (issue #8). One source of truth: the roster import
//! (#24) reads the column names, the Students pages (#22) read the types,
//! labels and list flags, a Module's Mini App page the `student_visible`
//! ones, and both validate values with [`AttributeSpec::parse`].
//!
//! ```toml
//! [roster]
//! student_code = "รหัสนักศึกษา"
//! display_name = "ชื่อ-สกุล"
//! email        = "อีเมล"
//! locale       = { column = "ภาษา", default = "th" }
//! tags         = { column = "กลุ่ม", separator = ";" }
//!
//! [roster.attributes.year]
//! column = "ชั้นปี"
//! type = "integer"
//! label = { th = "ชั้นปี", en = "Year" }
//! shown_in_list = true
//! student_visible = true
//! order = 2
//!
//! [roster.attributes.programme]
//! column = "หลักสูตร"
//! type = "enum"
//! values = ["MSC", "MSC-Intl"]
//! label = { th = "หลักสูตร", en = "Programme" }
//! shown_in_list = true
//! order = 1
//! ```

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value as Json;

use crate::message::Locale;

mod date;
pub(crate) use date::is_date;

/// `[roster]`. Every field has a default, so a Deployment without a roster
/// file still parses (no attributes, columns named like the fields).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct RosterConfig {
    /// Header of the student id column (`student.student_code`).
    pub student_code: String,
    /// Header of the name column (`student.display_name`).
    pub display_name: String,
    /// Header of the email column (`student.email`; an empty cell is
    /// allowed: such Students need a Manual Link).
    pub email: String,
    /// The Locale column, if the file has one.
    pub locale: Option<LocaleColumn>,
    /// The roster Tags column, if the file has one.
    pub tags: Option<TagsColumn>,
    /// The attribute schema of `student.attributes`, keyed by attribute
    /// key (the JSON key and the `manual_overrides` entry
    /// `attributes.<key>`). Shown sorted by [`AttributeSpec::order`],
    /// then key.
    pub attributes: BTreeMap<String, AttributeSpec>,
    /// `true` when parsed from a `[roster]` section (not a TOML key); see
    /// [`RosterConfig::is_mapped`].
    #[serde(skip_deserializing, default = "section_present")]
    pub(crate) declared: bool,
}

fn section_present() -> bool {
    true
}

impl Default for RosterConfig {
    fn default() -> Self {
        Self {
            student_code: "student_code".into(),
            display_name: "display_name".into(),
            email: "email".into(),
            locale: None,
            tags: None,
            attributes: BTreeMap::new(),
            declared: false,
        }
    }
}

/// `locale = { column = "..", default = "th" }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct LocaleColumn {
    /// Header of the column.
    pub column: String,
    /// Locale for an empty cell; the Deployment's default Locale if absent.
    #[serde(default)]
    pub default: Option<Locale>,
}

/// `tags = { column = "..", separator = ";" }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct TagsColumn {
    /// Header of the column.
    pub column: String,
    /// Separator between Tag names in one cell (default `;`).
    #[serde(default = "default_separator")]
    pub separator: String,
}

fn default_separator() -> String {
    ";".into()
}

/// One attribute of `student.attributes`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct AttributeSpec {
    /// Header of the roster column holding it.
    pub column: String,
    /// Value type.
    #[serde(rename = "type")]
    pub kind: AttributeType,
    /// Allowed values of an `enum` (required for `enum`, refused otherwise).
    #[serde(default)]
    pub values: Vec<String>,
    /// Label per Locale (`label = { th = "..", en = ".." }`); the key when
    /// none fits.
    #[serde(default)]
    pub label: BTreeMap<String, String>,
    /// May be empty. Default `false`: a required attribute. A required
    /// attribute must have a value in every roster row and whenever Staff
    /// edit that field; the edit form re-checks only the fields Staff
    /// changed, so a Student stored without it can still be edited.
    #[serde(default)]
    pub optional: bool,
    /// Shown on the Students list (cards and table). Default `false`.
    #[serde(default)]
    pub shown_in_list: bool,
    /// Given to a Module's Mini App page (`AppStudent::attributes`), so the
    /// Student may see it. Default `false`: an attribute is staff-only
    /// until the Deployment opts in.
    #[serde(default)]
    pub student_visible: bool,
    /// Display order, lower first (default 0; ties by key).
    #[serde(default)]
    pub order: u16,
}

/// The type of an attribute value, as stored in `student.attributes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum AttributeType {
    /// Any text (JSON string).
    Text,
    /// A whole number (JSON integer), e.g. a year.
    Integer,
    /// A decimal number (JSON number).
    Number,
    /// A calendar date `YYYY-MM-DD` (JSON string).
    Date,
    /// One of [`AttributeSpec::values`] (JSON string).
    Enum,
}

/// Why a value does not fit its [`AttributeSpec`]. [`AttrError::key`] is
/// the catalog key of the message shown to Staff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttrError {
    /// Empty, and the attribute is not optional.
    Required,
    /// Not a whole number.
    NotInteger,
    /// Not a number.
    NotNumber,
    /// Not a `YYYY-MM-DD` date that exists.
    NotDate,
    /// Not one of the enum values.
    NotInList,
}

impl AttrError {
    /// Catalog key of the message (`kernel.students.err_*`).
    pub fn key(self) -> &'static str {
        match self {
            AttrError::Required => "kernel.students.err_required",
            AttrError::NotInteger => "kernel.students.err_integer",
            AttrError::NotNumber => "kernel.students.err_number",
            AttrError::NotDate => "kernel.students.err_date",
            AttrError::NotInList => "kernel.students.err_enum",
        }
    }
}

impl AttributeSpec {
    /// The label in `locale`, else in `fallback`, else `key`.
    pub fn label_in<'a>(&'a self, key: &'a str, locale: &Locale, fallback: &Locale) -> &'a str {
        self.label
            .get(&locale.0)
            .or_else(|| self.label.get(&fallback.0))
            .map_or(key, String::as_str)
    }

    /// Parses a raw cell or form value (trimmed): `Ok(None)` for an empty
    /// optional value, `Ok(Some(json))` for the value to store.
    pub fn parse(&self, raw: &str) -> Result<Option<Json>, AttrError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return if self.optional {
                Ok(None)
            } else {
                Err(AttrError::Required)
            };
        }
        let v = match self.kind {
            AttributeType::Text => Json::String(raw.into()),
            AttributeType::Integer => {
                Json::from(raw.parse::<i64>().map_err(|_| AttrError::NotInteger)?)
            }
            AttributeType::Number => {
                let n: f64 = raw.parse().map_err(|_| AttrError::NotNumber)?;
                serde_json::Number::from_f64(n)
                    .map(Json::Number)
                    .ok_or(AttrError::NotNumber)?
            }
            AttributeType::Date if is_date(raw) => Json::String(raw.into()),
            AttributeType::Date => return Err(AttrError::NotDate),
            AttributeType::Enum if self.values.iter().any(|v| v == raw) => Json::String(raw.into()),
            AttributeType::Enum => return Err(AttrError::NotInList),
        };
        Ok(Some(v))
    }

    /// A stored value as form / cell text (`""` for none).
    pub fn display(value: Option<&Json>) -> String {
        match value {
            None | Some(Json::Null) => String::new(),
            Some(Json::String(s)) => s.clone(),
            Some(other) => other.to_string(),
        }
    }
}

impl RosterConfig {
    /// Whether roster files can be imported (#24): a `[roster]` section,
    /// or a mapping changed in code from the defaults.
    pub fn is_mapped(&self) -> bool {
        self.declared || *self != Self::default()
    }

    /// The attributes in display order: `(key, spec)`.
    pub fn attributes_in_order(&self) -> Vec<(&str, &AttributeSpec)> {
        let mut v: Vec<(&str, &AttributeSpec)> = self
            .attributes
            .iter()
            .map(|(k, s)| (k.as_str(), s))
            .collect();
        v.sort_by_key(|(k, s)| (s.order, *k));
        v
    }

    /// Refuses bad keys (not `[a-z][a-z0-9_]*`, at most 40 bytes), empty or
    /// repeated column headers, an `enum` without values (or with empty or
    /// repeated ones), `values` on a non-`enum`, and an empty separator.
    pub(crate) fn check(&self) -> crate::Result<()> {
        let bad = |m: String| Err(crate::Error::Invalid(format!("[roster] {m}")));
        let mut columns = vec![&self.student_code, &self.display_name, &self.email];
        columns.extend(self.locale.as_ref().map(|l| &l.column));
        columns.extend(self.tags.as_ref().map(|t| &t.column));
        if self.tags.as_ref().is_some_and(|t| t.separator.is_empty()) {
            return bad("tags separator is empty".into());
        }
        for (key, a) in &self.attributes {
            let ok_key = key.len() <= 40
                && key.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
                && key
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if !ok_key {
                return bad(format!("attribute key `{key}` must match [a-z][a-z0-9_]*"));
            }
            let is_enum = a.kind == AttributeType::Enum;
            if is_enum && a.values.is_empty() {
                return bad(format!("attribute `{key}`: enum needs values"));
            }
            if !is_enum && !a.values.is_empty() {
                return bad(format!("attribute `{key}`: values only fit an enum"));
            }
            let mut seen = std::collections::BTreeSet::new();
            if a.values
                .iter()
                .any(|v| v.trim().is_empty() || !seen.insert(v))
            {
                return bad(format!("attribute `{key}`: empty or repeated enum value"));
            }
            columns.push(&a.column);
        }
        let mut seen = std::collections::BTreeSet::new();
        for c in columns {
            if c.trim().is_empty() || !seen.insert(c.trim()) {
                return bad(format!("column header `{c}` is empty or used twice"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: AttributeType, values: &[&str], optional: bool) -> AttributeSpec {
        AttributeSpec {
            column: "c".into(),
            kind,
            values: values.iter().map(|v| v.to_string()).collect(),
            label: BTreeMap::new(),
            optional,
            shown_in_list: false,
            student_visible: false,
            order: 0,
        }
    }

    #[test]
    fn values_are_parsed_by_type() {
        let int = spec(AttributeType::Integer, &[], false);
        assert_eq!(int.parse(" 3 "), Ok(Some(Json::from(3))));
        assert_eq!(int.parse("3.5"), Err(AttrError::NotInteger));
        assert_eq!(int.parse(""), Err(AttrError::Required));
        let num = spec(AttributeType::Number, &[], true);
        assert_eq!(num.parse("3.5"), Ok(Some(Json::from(3.5))));
        assert_eq!(num.parse("x"), Err(AttrError::NotNumber));
        assert_eq!(num.parse("NaN"), Err(AttrError::NotNumber));
        assert_eq!(num.parse(""), Ok(None));
        let en = spec(AttributeType::Enum, &["MSC", "MSC-Intl"], false);
        assert_eq!(en.parse("MSC-Intl"), Ok(Some(Json::from("MSC-Intl"))));
        assert_eq!(en.parse("msc"), Err(AttrError::NotInList));
        let date = spec(AttributeType::Date, &[], false);
        assert_eq!(date.parse("2028-02-29"), Ok(Some(Json::from("2028-02-29"))));
        for bad in [
            "2027-02-29",
            "2026-13-01",
            "2026-1-01",
            "26-01-01x",
            "2026/01/01",
        ] {
            assert_eq!(date.parse(bad), Err(AttrError::NotDate), "{bad}");
        }
    }

    #[test]
    fn schema_errors_are_refused() {
        let base = "name = \"X\"\ndefault_locale = \"th\"\n";
        let ok = "[roster.attributes.year]\ncolumn = \"ชั้นปี\"\ntype = \"integer\"\n\
                  label = { th = \"ชั้นปี\", en = \"Year\" }";
        let c = crate::DeploymentConfig::from_toml(&format!("{base}{ok}")).unwrap();
        let year = &c.roster.attributes["year"];
        assert_eq!(year.label_in("year", &Locale::en(), &Locale::th()), "Year");
        assert_eq!(
            year.label_in("year", &Locale("fr".into()), &Locale::th()),
            "ชั้นปี"
        );
        for extra in [
            "[roster.attributes.Year]\ncolumn = \"a\"\ntype = \"text\"",
            "[roster.attributes.p]\ncolumn = \"a\"\ntype = \"enum\"",
            "[roster.attributes.p]\ncolumn = \"a\"\ntype = \"text\"\nvalues = [\"x\"]",
            "[roster.attributes.p]\ncolumn = \"a\"\ntype = \"enum\"\nvalues = [\"x\", \"x\"]",
            "[roster.attributes.p]\ncolumn = \"email\"\ntype = \"text\"",
            "[roster.attributes.p]\ncolumn = \"a\"\ntype = \"bool\"",
            "[roster]\ntags = { column = \"g\", separator = \"\" }",
        ] {
            assert!(
                crate::DeploymentConfig::from_toml(&format!("{base}{extra}")).is_err(),
                "accepted: {extra}"
            );
        }
    }
}
