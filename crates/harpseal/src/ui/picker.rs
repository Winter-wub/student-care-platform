//! The Student picker (#146): a form field of student codes that Staff fill
//! by searching (a code's start or part of a name), by pasting many codes
//! at once, or both, and see each pick as a chip, with unknown and left
//! Students flagged and the count against the limit.
//!
//! **For Module authors.** Mount the picker's route under your own
//! Permission with [`crate::ModuleRegistrar::student_picker`], then render
//! a [`StudentPicker`] with the same path inside your `<form>`:
//!
//! ```ignore
//! const PICK: &str = "/reminders/pick";
//! r.student_picker(PERMISSION.id, PICK);
//! // In the page handler:
//! let field = ui::StudentPicker::new("ids", PICK, &t("reminders.ids"))
//!     .max(40)
//!     .codes(ids)
//!     .render(&s, &staff)
//!     .await?;
//! ```
//!
//! The form posts the field as before: one newline-separated text of
//! codes under the field's name, so the handler reads it with
//! [`crate::audience::parse_student_ids`] and must still check the limit
//! and the codes itself (the picker only shows them).
//!
//! **Without JS** (or when HTMX does not load) the field is a plain
//! `<textarea name="{field}">` of codes, as the reminders form was before.
//! A small inline script swaps in the picker once HTMX is there: the
//! textarea is disabled (so it is not posted) and a hidden input of the
//! same name carries the picked codes.
//!
//! **Who sees what.** The route answers only Staff holding the Permission
//! the Module named. Names are searched and shown only to Staff who also
//! hold `kernel.student.view`; others search by code and see codes, as the
//! reminders review does (#135). Every value is escaped by maud.

use maud::{Markup, PreEscaped, html};

use super::picker_route::{Resolved, resolve};
use crate::{CurrentStaff, Error, PageState, Result};

/// Who may see Students' names in the picker (the Students list's
/// Permission).
pub(crate) const NAMES: &str = "kernel.student.view";
/// Most characters of a field name ([`StudentPicker::new`]).
const MAX_FIELD: usize = 32;

/// A Student picker field (#146): search by a code's start or part of a
/// name, chips, paste many codes, unknown and left codes flagged, the
/// count against [`Self::max`]. Without JS it is a plain textarea of
/// codes. Mount its route with [`crate::ModuleRegistrar::student_picker`]
/// under your Permission, then render it inside your `<form>`:
///
/// ```ignore
/// const PICK: &str = "/reminders/pick";
/// r.student_picker(PERMISSION.id, PICK);
/// // In the page handler:
/// let field = ui::StudentPicker::new("ids", PICK, &t("reminders.ids"))
///     .max(40)
///     .codes(ids)
///     .render(&s, &staff)
///     .await?;
/// ```
///
/// The form posts newline-separated codes under the field's name; read
/// them with [`crate::audience::parse_student_ids`] and check the limit
/// and the codes yourself. Names are searched and shown only to Staff who
/// also hold `kernel.student.view`.
#[derive(Debug, Clone)]
pub struct StudentPicker<'a> {
    field: &'a str,
    path: &'a str,
    label: &'a str,
    help: Option<&'a str>,
    max: usize,
    codes: Vec<String>,
}

impl<'a> StudentPicker<'a> {
    /// A picker posting its codes under `field` (also the textarea's `id`;
    /// lowercase ASCII letters, digits and `_`, at most 32), searching
    /// through the route mounted at `path` with
    /// [`crate::ModuleRegistrar::student_picker`] (the same path, without
    /// `/admin`), and labelled `label`. Limit
    /// [`crate::audience::MAX_STUDENTS`] until [`Self::max`].
    pub fn new(field: &'a str, path: &'a str, label: &'a str) -> Self {
        Self {
            field,
            path,
            label,
            help: None,
            max: crate::audience::MAX_STUDENTS,
            codes: Vec::new(),
        }
    }

    /// A line under the label, also read with the field.
    pub fn help(mut self, help: &'a str) -> Self {
        self.help = Some(help);
        self
    }

    /// At most `max` Students (1 to [`crate::audience::MAX_STUDENTS`]):
    /// the picker counts against it and flags more. The form handler must
    /// refuse more itself.
    pub fn max(mut self, max: usize) -> Self {
        self.max = max.clamp(1, crate::audience::MAX_STUDENTS);
        self
    }

    /// The codes already picked, in order (duplicates are dropped).
    pub fn codes(mut self, codes: Vec<String>) -> Self {
        self.codes = codes;
        self
    }

    /// The field, with each picked code looked up once. An invalid field
    /// name is [`Error::Invalid`].
    pub async fn render(&self, s: &PageState, staff: &CurrentStaff) -> Result<Markup> {
        if !field_ok(self.field) {
            return Err(Error::Invalid(format!(
                "picker field {:?}: use a-z, 0-9 and _, at most {MAX_FIELD}",
                self.field
            )));
        }
        let codes = dedup(&self.codes, crate::audience::MAX_STUDENTS);
        let found = resolve(s, &codes).await?;
        let names = staff.can(NAMES);
        let t = |k: &str| s.catalog.t(&staff.locale, k);
        let f = self.field;
        let url = format!("/admin{}", self.path);
        let vals = serde_json::json!({ "pf": f, "pmax": self.max.to_string() }).to_string();
        let help_id = format!("{f}-help");
        let search_key = if names {
            "kernel.picker.search"
        } else {
            "kernel.picker.search_code"
        };
        Ok(html! {
            div class="field picker" id={ (f) "-picker" } hx-vals=(vals) {
                label for=(f) id={ (f) "-label" } { (self.label) }
                @if let Some(h) = self.help { p class="muted" id=(help_id) { (h) } }
                textarea id=(f) name=(f) rows="6"
                    aria-describedby=[self.help.map(|_| help_id.as_str())] { (codes.join("\n")) }
                div class="picker-js" hidden {
                    div id={ (f) "-picked" } {
                        (picked(s, staff, &url, f, self.max, &codes, &found, true))
                    }
                    p class="picker-status" id={ (f) "-status" } role="status" {}
                    label for={ (f) "-q" } { (t(search_key)) }
                    p class="muted" id={ (f) "-hint" } { (t("kernel.picker.hint")) }
                    // `form` names no form: Enter here searches and never
                    // submits the page's form.
                    input id={ (f) "-q" } type="search" name="q" form={ (f) "-detached" }
                        autocomplete="off" aria-describedby={ (f) "-hint" }
                        aria-controls={ (f) "-results" }
                        hx-get=(url) hx-trigger="input changed delay:250ms, search"
                        hx-target={ "#" (f) "-results" } hx-params="q,pf,pmax";
                    div class="picker-results" id={ (f) "-results" } aria-live="polite" {}
                    details class="picker-paste" {
                        summary { (t("kernel.picker.paste")) }
                        label for={ (f) "-paste" } id={ (f) "-paste-label" } { (t("kernel.picker.paste_label")) }
                        textarea id={ (f) "-paste" } name="paste" form={ (f) "-detached" } rows="4" {}
                        div class="actions" {
                            @let add = t("kernel.picker.paste_add");
                            (edit_button(&url, f, "paste", "", &add, &add, "secondary"))
                        }
                    }
                }
                script { (PreEscaped(enhance_js(f))) }
            }
        })
    }
}

/// Whether `f` is a usable field name: it goes into ids and selectors.
pub(crate) fn field_ok(f: &str) -> bool {
    !f.is_empty()
        && f.len() <= MAX_FIELD
        && f.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// `codes` without duplicates or empty ones, at most `cap`.
pub(crate) fn dedup(codes: &[String], cap: usize) -> Vec<String> {
    let joined = codes.join("\n");
    let mut out = crate::audience::parse_student_ids(&joined);
    out.truncate(cap);
    out
}

/// A button that posts one change (`op`: `add`, `remove`, `paste`,
/// `clear`) to the route and swaps the picked list.
pub(crate) fn edit_button(
    url: &str,
    f: &str,
    op: &str,
    code: &str,
    label: &str,
    aria: &str,
    class: &str,
) -> Markup {
    let vals = serde_json::json!({ "op": op, "code": code }).to_string();
    let include = if op == "paste" {
        format!("#{f}-value, #{f}-paste")
    } else {
        format!("#{f}-value")
    };
    html! {
        button type="button" class=(class) aria-label=[(aria != label).then_some(aria)] hx-post=(url) hx-vals=(vals)
            hx-include=(include) hx-params={ "pf,pmax,op,code,paste," (f) }
            hx-target={ "#" (f) "-picked" } hx-swap="innerHTML" { (label) }
    }
}

/// The picked codes: the hidden value (disabled until the script turns the
/// picker on), the count, the flags and one chip per code. `first`: the
/// page's own render, whose hidden input the script enables; a swapped-in
/// one is live at once.
#[allow(clippy::too_many_arguments)]
pub(crate) fn picked(
    s: &PageState,
    staff: &CurrentStaff,
    url: &str,
    f: &str,
    max: usize,
    codes: &[String],
    found: &Resolved,
    first: bool,
) -> Markup {
    let tf = |k: &str, a: &[(&str, &str)]| s.catalog.tf(&staff.locale, k, a);
    let t = |k: &str| tf(k, &[]);
    let names = staff.can(NAMES);
    let unknown = codes.iter().filter(|c| !found.contains_key(*c)).count();
    let left = codes
        .iter()
        .filter(|c| found.get(*c).is_some_and(|r| r.left))
        .count();
    let (n, max_s) = (super::number(codes.len() as i64), super::number(max as i64));
    let over = codes.len().saturating_sub(max);
    html! {
        input type="hidden" id={ (f) "-value" } name=(f) value=(codes.join("\n")) disabled[first];
        p class="picker-count" {
            strong { (tf("kernel.picker.count", &[("n", &n), ("max", &max_s)])) }
        }
        @if over > 0 {
            p class="field-error" { (tf("kernel.picker.over", &[("max", &max_s), ("n", &super::number(over as i64))])) }
        }
        @if unknown > 0 {
            p class="field-error" { (tf("kernel.picker.unknown_n", &[("n", &super::number(unknown as i64))])) }
        }
        @if left > 0 {
            p class="field-warning" { (tf("kernel.picker.left_n", &[("n", &super::number(left as i64))])) }
        }
        @if codes.is_empty() {
            p class="muted" { (t("kernel.picker.empty")) }
        } @else {
            ul class="picker-chips" aria-labelledby={ (f) "-label" } {
                @for c in codes {
                    @let r = found.get(c);
                    @let class = match r { None => "picker-chip err", Some(r) if r.left => "picker-chip warn", _ => "picker-chip" };
                    li class=(class) {
                        span class="picker-code" { (c) }
                        @if let Some(name) = r.and_then(|r| r.name.as_deref()).filter(|_| names) {
                            " " span class="picker-name" { (name) }
                        }
                        @match r {
                            None => { " " span class="picker-flag" { (t("kernel.picker.unknown")) } }
                            Some(r) if r.left => { " " span class="picker-flag" { (t("kernel.picker.left")) } }
                            _ => {}
                        }
                        @let aria = tf("kernel.picker.remove", &[("code", c)]);
                        (edit_button(url, f, "remove", c, &t("kernel.picker.remove_short"), &aria, "link"))
                    }
                }
            }
            div class="actions" {
                @let clear = t("kernel.picker.clear");
                (edit_button(url, f, "clear", "", &clear, &clear, "link"))
            }
        }
    }
}

/// The script that turns the picker on once HTMX has loaded: it disables
/// the plain textarea (no longer posted), enables the hidden value, shows
/// the picker and points the label at the search box. After a chip is
/// removed (its button gone) focus goes back to the search box.
fn enhance_js(f: &str) -> String {
    // `f` is checked by `field_ok`: only a-z, 0-9 and `_`.
    format!(
        r#"(function(){{var f="{f}";function on(){{if(!window.htmx)return;var g=function(x){{return document.getElementById(f+x);}};
var t=document.getElementById(f),w=g("-picker");if(!t||!w)return;t.disabled=true;t.hidden=true;
var v=g("-value");if(v)v.disabled=false;w.querySelector(".picker-js").hidden=false;g("-label").htmlFor=f+"-q";
document.body.addEventListener("htmx:afterSwap",function(e){{if(e.target===g("-picked")&&(!document.activeElement||document.activeElement===document.body))g("-q").focus();}});}}
if(document.readyState==="loading")document.addEventListener("DOMContentLoaded",on);else on();}})();"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_names_stay_safe_for_ids_selectors_and_the_script() {
        for ok in ["ids", "students_2", "a"] {
            assert!(field_ok(ok), "{ok}");
        }
        for bad in [
            "",
            "Ids",
            "a-b",
            "a b",
            "a\"",
            "x</script>",
            &"a".repeat(33),
        ] {
            assert!(!field_ok(bad), "{bad}");
        }
        let js = enhance_js("ids");
        assert!(!js.contains("</"), "nothing closes the inline script early");
        assert!(js.contains("t.disabled=true") && js.contains("v.disabled=false"));
    }

    #[test]
    fn duplicates_and_blanks_go_and_the_list_is_capped() {
        let codes = ["1", " 2 ", "1", "", "3"].map(String::from).to_vec();
        assert_eq!(dedup(&codes, 10), ["1", "2", "3"]);
        assert_eq!(dedup(&codes, 2), ["1", "2"]);
    }
}
