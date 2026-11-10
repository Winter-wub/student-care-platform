//! The explicit Audience choice on the form (#120): ทุกคน / เลือกกลุ่ม /
//! เลือกรายคน, none preselected on a new announcement. Each choice shows
//! only its own fields (#156, [`SCRIPT`]); without JS every filter stays
//! visible. The server reads the choice either way
//! ([`super::form::Fields::scope_problem`]) and drops the filters of the
//! other choices.

use maud::{Markup, html};

use super::form::Fields;
use super::view::View;

pub(super) const CHOICES: [&str; 3] = ["everyone", "groups", "students"];

/// Shows the parts (`[data-scope-for]`) of the checked choice and hides
/// the others, on a change of choice and whenever HTMX swaps the fieldset
/// in. No choice: neither part; a combined Audience (the note): both.
pub(super) const SCRIPT: &str = r#"(function(){if(window.hsScope)return;window.hsScope=1;
function apply(fs){if(!fs)return;var c=fs.querySelector('input[name=scope]:checked');var all=!c&&fs.querySelector('.scope [role=note]');
fs.querySelectorAll('[data-scope-for]').forEach(function(p){p.hidden=all?false:!(c&&c.value===p.getAttribute('data-scope-for'));});}
document.addEventListener('change',function(e){var t=e.target;if(t&&t.name==='scope')apply(t.closest('#audience'));});
document.addEventListener('htmx:load',function(e){var r=e.detail.elt;if(!r||!r.querySelector)return;apply(r.id==='audience'?r:r.querySelector('#audience'));});
var go=function(){apply(document.getElementById('audience'));};
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',go);else go();})();"#;

/// Whether the part for `part` starts hidden: only in an HTMX answer
/// (`scripted`, so [`SCRIPT`] runs) with another choice made.
pub(super) fn hidden(f: &Fields, part: &str, scripted: bool) -> bool {
    scripted && CHOICES.contains(&f.scope.as_str()) && f.scope != part
}

/// A combined Audience (groups AND a list, saved before #120) has no
/// choice made and no `required`: saved as it is, it keeps both filters;
/// the note says what each choice would drop.
pub(super) fn field(v: &View<'_>, f: &Fields) -> Markup {
    let combined = f.combined();
    html! {
        fieldset class="scope" {
            legend { (v.t("announcements.form.scope_legend")) }
            @if combined {
                p class="note warn-note" role="note" { (v.t("announcements.form.scope_combined")) }
            }
            @for c in CHOICES {
                label class="check" {
                    input type="radio" name="scope" value=(c) required[!combined] checked[f.scope == c];
                    " " (v.t(&format!("announcements.form.scope_{c}")))
                }
            }
        }
    }
}

/// The fields of a new announcement from its query (#127): `scope=students`
/// with `students=<ids separated by spaces>` (the next step of an import
/// that names its Students, [`crate::ui::announce_href`]) chooses that
/// list; any other query chooses nothing, as on a bare form (#120). Only a
/// draft is filled in: it is saved and reviewed as usual before any send.
pub(super) fn prefilled(q: &[(String, String)]) -> Fields {
    let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, x)| x.trim());
    let named = get("students").filter(|l| get("scope") == Some("students") && !l.is_empty());
    let (scope, students) = match named {
        Some(list) => {
            let ids: Vec<&str> = list.split_whitespace().collect();
            ("students".to_string(), ids.join("\n"))
        }
        None => (String::new(), String::new()),
    };
    Fields {
        when: "now".into(),
        scope,
        students,
        ..Fields::default()
    }
}

/// The student codes of a "ประกาศถึงที่เลือก" over a whole Students list
/// filter (#145), too many for a link to carry: `from=students` with the
/// list's `q` and `missing`, read again here
/// ([`crate::kernel::students::codes_matching`]). `None` (a bare form)
/// without `kernel.student.view`, when nobody or more than 2,000 Students
/// match, or when the read fails. Only a draft is filled in.
pub(super) async fn from_students(
    s: &crate::AppState,
    staff: &crate::CurrentStaff,
    q: &[(String, String)],
) -> Option<Vec<String>> {
    let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, x)| x.as_str());
    if get("from") != Some("students") || !staff.can("kernel.student.view") {
        return None;
    }
    let missing = matches!(get("missing"), Some("1" | "on" | "true"));
    let roster = &s.config.roster;
    crate::kernel::students::codes_matching(&s.services, roster, get("q").unwrap_or(""), missing)
        .await
        .ok()
        .flatten()
}

/// The errors of `field` under one id, `err-<field>`, that the field's
/// control or fieldset names with `aria-describedby` (#129). Not a live
/// region: the form's summary above announces once.
pub(super) fn field_errors(v: &View<'_>, errors: &[(&str, &str)], field: &str) -> Markup {
    let mine: Vec<&str> = errors
        .iter()
        .filter(|(f, _)| *f == field)
        .map(|(_, k)| *k)
        .collect();
    html! {
        @if !mine.is_empty() {
            div id=(format!("err-{field}")) { @for key in mine { p class="field-error" { (v.t(key)) } } }
        }
    }
}

/// `err-<field>` when `field` has an error, for `aria-describedby`.
pub(super) fn described(errors: &[(&str, &str)], field: &str) -> Option<String> {
    errors
        .iter()
        .any(|(f, _)| *f == field)
        .then(|| format!("err-{field}"))
}
