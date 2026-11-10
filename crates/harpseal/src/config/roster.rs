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
//! # Which import modes Staff may choose, and the one the form preselects
//! # and a request without a mode uses. Default: both; without a
//! # `default_mode`, `upsert` when listed (#120: a full replace is a
//! # deliberate choice), else the only mode listed.
//! modes        = ["full", "upsert"]
//! default_mode = "upsert"
//! # Warning classes that block the file (#71): `missing_email`,
//! # `invalid_email`, `duplicate_email`, `empty_attribute`. Default: none.
//! blocking     = ["invalid_email"]
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
use serde_json::{Number, Value as Json};

use crate::message::Locale;

mod check;
mod date;
mod rules;
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
    /// The import modes Staff may choose (#79): `full` (Students missing
    /// from the file become `left`) and/or `upsert` (add and update only).
    /// A Deployment whose files never list every Student (one file per
    /// cohort) lists `["upsert"]` only. Default: both.
    pub modes: Vec<String>,
    /// The mode the upload form preselects and a request without a mode
    /// uses; one of [`RosterConfig::modes`]. When the TOML leaves it out:
    /// `upsert` if `modes` lists it (#120: a full replace, which makes
    /// Students left, is a deliberate choice), else the only mode listed;
    /// `modes` with several entries and no `upsert` must name it. Empty
    /// (unset) in [`RosterConfig::default`]; filled when the Deployment
    /// is built, for a config made in code too.
    pub default_mode: String,
    /// Warning classes this Deployment makes blocking (#71): `missing_email`,
    /// `invalid_email`, `duplicate_email`, `empty_attribute`. A file with one
    /// is refused like one with a malformed id. Default: none, so each only
    /// warns, except `empty_attribute` (a blank optional attribute cell),
    /// which since #134 raises nothing unless listed here.
    pub blocking: Vec<String>,
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
            modes: vec!["full".into(), "upsert".into()],
            default_mode: String::new(),
            blocking: Vec::new(),
            attributes: BTreeMap::new(),
            declared: false,
        }
    }
}

/// The problem classes of a roster file that only warn unless the
/// Deployment lists them in `blocking` (#71). Every other
/// problem (an empty or malformed id, a duplicate id, an unreadable value,
/// a required value missing) always blocks.
pub const WARNING_CLASSES: [&str; 4] = [
    "missing_email",
    "invalid_email",
    "duplicate_email",
    "empty_attribute",
];

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
    /// Smallest value of an `integer` or `number`, inclusive (#81).
    #[serde(default)]
    pub min: Option<Number>,
    /// Largest value of an `integer` or `number`, inclusive (#81).
    #[serde(default)]
    pub max: Option<Number>,
    /// Display labels of an `enum`'s values, per Locale (#81):
    /// `value_labels = { current = { th = "..", en = ".." } }`. Stored data
    /// and filters keep the value.
    #[serde(default)]
    pub value_labels: BTreeMap<String, BTreeMap<String, String>>,
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

impl AttributeType {
    /// `integer` or `number`: the types an Audience range (#156) takes.
    pub fn is_numeric(self) -> bool {
        matches!(self, Self::Integer | Self::Number)
    }
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
    /// Below `min` or above `max` (#81).
    OutOfRange,
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
            AttrError::OutOfRange => "kernel.students.err_range",
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
            AttributeType::Integer | AttributeType::Number => match self.number(raw)? {
                Some(v) if self.in_range(&v) => v,
                Some(_) => return Err(AttrError::OutOfRange),
                None if self.optional => return Ok(None),
                None => return Err(AttrError::Required),
            },
            AttributeType::Date if is_date(raw) => Json::String(raw.into()),
            AttributeType::Date => return Err(AttrError::NotDate),
            AttributeType::Enum if self.values.iter().any(|v| v == raw) => Json::String(raw.into()),
            AttributeType::Enum => return Err(AttrError::NotInList),
        };
        Ok(Some(v))
    }

    /// An `integer` or `number` value read with [`crate::sheet`]
    /// (`12,500`, `12,500.00`); a lone `-` is no value. An integer is
    /// read exactly ([`crate::sheet::number_scaled`] at scale 0: whole,
    /// within `i64`, never through `f64`); a number as an `f64` of at most
    /// 15 significant digits.
    fn number(&self, raw: &str) -> Result<Option<Json>, AttrError> {
        use crate::sheet::{Blank, number, number_scaled};
        if self.kind == AttributeType::Integer {
            let n = number_scaled(raw, 0, Blank::Missing).map_err(|_| AttrError::NotInteger)?;
            return Ok(n.map(Json::from));
        }
        match number(raw, Blank::Missing).map_err(|_| AttrError::NotNumber)? {
            None => Ok(None),
            Some(n) => serde_json::Number::from_f64(n)
                .map(|n| Some(Json::Number(n)))
                .ok_or(AttrError::NotNumber),
        }
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
        // The default `default_mode` once the build filled it is still the
        // default (#120).
        let mut d = Self::default();
        if self.default_mode == "upsert" {
            d.resolve_default_mode();
        }
        self.declared || *self != d
    }

    /// Whether `mode` is one of [`RosterConfig::modes`].
    pub fn allows_mode(&self, mode: &str) -> bool {
        self.modes.iter().any(|m| m == mode)
    }

    /// Whether warning class `class` is made blocking (#71).
    pub fn blocks(&self, class: &str) -> bool {
        self.blocking.iter().any(|c| c == class)
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
}

#[cfg(test)]
#[path = "roster_tests.rs"]
mod tests;
