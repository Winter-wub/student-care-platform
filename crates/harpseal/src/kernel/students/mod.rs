//! The Students service and back-office pages (issue #22).
//!
//! Pages (Kernel back-office routes under `/admin/students`):
//! list and HTMX search (`kernel.student.view`), the detail page
//! (`kernel.student.view`, logged as the read `student.view`), the edit
//! form, "ใช้ค่าจากทะเบียน", manual Tags and the status change
//! (`kernel.student.edit`), Manual Link (`kernel.link.manage`) and revoke
//! (`kernel.link.revoke`, Staff unlink), both through [`crate::links`].
//!
//! **Manual edits win over the roster** (#8): a field Staff edit by hand is
//! listed in `student.manual_overrides` (`display_name`, `email`,
//! `locale`, `status`, `attributes.<key>`) and a roster import keeps its value: the
//! import calls [`merge_import`]. [`clear_override`] takes a field off the
//! list, so the next import sets it again.
//!
//! Every write is one `Db::batch` gated on its own audit row
//! ([`crate::audit::entry_if`]): the audit row is written only if the row
//! still holds what the page read (a compare-and-set), and the change runs
//! only if that audit row exists. A lost race is [`crate::Error::Conflict`].

mod actions;
mod detail;
mod edit;
mod form;
mod list;
mod read;
mod service;
mod tags;
pub(crate) mod texts;
pub(crate) mod view;

pub(crate) use read::search;

use axum::routing::{get, post};
use serde_json::{Map, Value as Json};

pub use service::{StatusChange, clear_override, record_view, set_status, update};
pub use tags::{add_manual_tag, remove_manual_tag};
pub(crate) use view::{channel_label, format_ms, is_htmx};

use crate::message::Locale;
use crate::module::ModuleRegistrar;

/// Core fields that can be overridden, besides `attributes.<key>`.
pub const CORE_OVERRIDES: [&str; 4] = ["display_name", "email", "locale", "status"];

/// The `manual_overrides` entry of attribute `key`.
pub fn attribute_field(key: &str) -> String {
    format!("attributes.{key}")
}

/// The fields of a Student a roster row sets and Staff may edit.
/// `student_code` is the identity, so it is not here.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StudentFields {
    /// `student.display_name`.
    pub display_name: String,
    /// `student.email`; `None` when the roster has none.
    pub email: Option<String>,
    /// `student.locale`.
    pub locale: Locale,
    /// `student.status`: `active` or `left`. An import sets `active` for a
    /// listed Student and `left` (full mode) for a missing one; Staff
    /// change it with [`set_status`], never with [`update`].
    pub status: String,
    /// `student.attributes`, per the `[roster.attributes]` schema.
    pub attributes: Map<String, Json>,
}

impl StudentFields {
    /// Fields of an `active` Student with no attributes.
    pub fn new(display_name: impl Into<String>, email: Option<String>, locale: Locale) -> Self {
        Self {
            display_name: display_name.into(),
            email,
            locale,
            status: "active".into(),
            attributes: Map::new(),
        }
    }

    /// The same fields with `status` set (`active` / `left`).
    pub fn with_status(mut self, status: &str) -> Self {
        self.status = status.into();
        self
    }

    /// The same fields with attribute `key` set to `value`.
    pub fn with_attribute(mut self, key: &str, value: impl Into<Json>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// The value of override field `field` (a core field or
    /// `attributes.<key>`); `Null` when absent.
    pub(crate) fn value_of(&self, field: &str) -> Json {
        match field {
            "display_name" => Json::from(self.display_name.clone()),
            "email" => self.email.clone().map_or(Json::Null, Json::from),
            "locale" => Json::from(self.locale.0.clone()),
            "status" => Json::from(self.status.clone()),
            other => other
                .strip_prefix("attributes.")
                .and_then(|k| self.attributes.get(k).cloned())
                .unwrap_or(Json::Null),
        }
    }
}

/// What [`merge_import`] decided for one Student.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Merge {
    /// The fields to store.
    pub fields: StudentFields,
    /// Override fields whose roster value differs from the stored one and
    /// was skipped (the preview's "skipped by manual override" count).
    pub skipped: Vec<String>,
}

/// The rule "manual edits win" (#8), as a pure function: the `incoming`
/// roster row replaces `existing`, except the fields listed in
/// `overrides`, which keep their `existing` value (a `status` set by hand
/// included: a Student Staff marked `left` stays left while listed). Attributes are merged
/// per key the same way (`attributes.<key>`): a non-overridden attribute
/// missing from the row is dropped, an overridden one is kept even when the
/// row lacks it. Entries of `overrides` that name no field are ignored.
pub fn merge_import(
    existing: &StudentFields,
    incoming: &StudentFields,
    overrides: &[String],
) -> Merge {
    let held = |f: &str| overrides.iter().any(|o| o == f);
    let mut fields = incoming.clone();
    let mut skipped = Vec::new();
    let mut note = |f: &str| {
        if existing.value_of(f) != incoming.value_of(f) {
            skipped.push(f.to_string());
        }
    };
    if held("display_name") {
        fields.display_name = existing.display_name.clone();
        note("display_name");
    }
    if held("email") {
        fields.email = existing.email.clone();
        note("email");
    }
    if held("locale") {
        fields.locale = existing.locale.clone();
        note("locale");
    }
    if held("status") {
        fields.status = existing.status.clone();
        note("status");
    }
    for o in overrides {
        let Some(key) = o.strip_prefix("attributes.") else {
            continue;
        };
        match existing.attributes.get(key) {
            Some(v) => fields.attributes.insert(key.to_string(), v.clone()),
            None => fields.attributes.remove(key),
        };
        note(o);
    }
    Merge { fields, skipped }
}

/// `ms` (UTC) as `YYYY-MM-DD HH:MM` at `offset_minutes` east, as the
/// Students pages show times (the roster import pages, #24, too).
pub(crate) fn format_time(ms: i64, offset_minutes: i32) -> String {
    view::format_ms(ms, offset_minutes)
}

/// Registers the pages on the Kernel.
pub(crate) fn register(r: &mut ModuleRegistrar) {
    const VIEW: &str = "kernel.student.view";
    const EDIT: &str = "kernel.student.edit";
    const LINK: &str = "kernel.link.manage";
    const REVOKE: &str = "kernel.link.revoke";
    r.messages(texts::DEFAULTS)
        .back_office_page(
            VIEW,
            "/students",
            "kernel.students.title",
            10,
            get(list::page),
        )
        .back_office(VIEW, "/students/{id}", get(detail::page))
        .back_office(
            EDIT,
            "/students/{id}/edit",
            get(edit::form).post(edit::submit),
        )
        .back_office(EDIT, "/students/{id}/override", post(edit::clear))
        .back_office(EDIT, "/students/{id}/tags", post(actions::add_tag))
        .back_office(
            EDIT,
            "/students/{id}/tags/remove",
            post(actions::remove_tag),
        )
        .back_office(EDIT, "/students/{id}/status", post(actions::status))
        .back_office(LINK, "/students/{id}/links", post(actions::manual_link))
        .back_office(
            REVOKE,
            "/students/{id}/links/{link_id}/revoke",
            post(actions::revoke),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, email: &str, year: i64) -> StudentFields {
        StudentFields::new(name, Some(email.into()), Locale::th()).with_attribute("year", year)
    }

    #[test]
    fn an_overridden_field_survives_the_import() {
        let existing = row("Nok (edited)", "nok@sealbay.example", 2);
        let incoming = row("Nok Thale", "nok@sealbay.example", 3);
        let m = merge_import(&existing, &incoming, &["display_name".into()]);
        assert_eq!(m.fields.display_name, "Nok (edited)");
        assert_eq!(m.skipped, vec!["display_name".to_string()]);
    }

    #[test]
    fn non_overridden_fields_take_the_import_value() {
        let existing = row("Nok", "old@sealbay.example", 2);
        let incoming = row("Nok Thale", "new@sealbay.example", 3);
        let m = merge_import(&existing, &incoming, &["attributes.year".into()]);
        assert_eq!(m.fields.display_name, "Nok Thale");
        assert_eq!(m.fields.email.as_deref(), Some("new@sealbay.example"));
        assert_eq!(m.fields.attributes["year"], Json::from(2));
        assert_eq!(m.skipped, vec!["attributes.year".to_string()]);
    }

    #[test]
    fn a_cleared_override_takes_the_import_value() {
        let existing = row("Nok (edited)", "nok@sealbay.example", 2);
        let incoming = row("Nok Thale", "nok@sealbay.example", 3);
        // "ใช้ค่าจากทะเบียน" emptied the list: the roster wins everywhere.
        let m = merge_import(&existing, &incoming, &[]);
        assert_eq!(m.fields, incoming);
        assert!(m.skipped.is_empty());
    }

    #[test]
    fn a_status_set_by_hand_survives_the_import() {
        let existing = row("Nok", "nok@sealbay.example", 2).with_status("left");
        let incoming = row("Nok", "nok@sealbay.example", 2);
        let m = merge_import(&existing, &incoming, &["status".into()]);
        assert_eq!(m.fields.status, "left");
        assert_eq!(m.skipped, vec!["status".to_string()]);
        assert_eq!(
            merge_import(&existing, &incoming, &[]).fields.status,
            "active"
        );
    }

    #[test]
    fn attributes_merge_per_key() {
        let existing = StudentFields::new("A", None, Locale::en())
            .with_attribute("note", "kept")
            .with_attribute("room", "B2");
        let incoming = StudentFields::new("A", None, Locale::th()).with_attribute("year", 1);
        let overrides = ["attributes.note".into(), "locale".into(), "bogus".into()];
        let m = merge_import(&existing, &incoming, &overrides);
        assert_eq!(m.fields.attributes["note"], Json::from("kept"));
        assert!(!m.fields.attributes.contains_key("room"), "not overridden");
        assert_eq!(m.fields.attributes["year"], Json::from(1));
        assert_eq!(m.fields.locale, Locale::en());
        assert_eq!(
            m.skipped,
            vec!["locale".to_string(), "attributes.note".into()]
        );
        // Same value on both sides: nothing to report as skipped.
        let same = merge_import(&existing, &existing, &overrides);
        assert!(same.skipped.is_empty());
    }
}
