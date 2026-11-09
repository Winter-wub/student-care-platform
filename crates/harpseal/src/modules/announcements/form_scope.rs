//! The explicit Audience choice on the form (#120): ทุกคน / เลือกกลุ่ม /
//! เลือกรายคน, none preselected on a new announcement. Without JS every
//! filter stays visible; the server reads the choice
//! ([`super::form::Fields::scope_problem`]).

use maud::{Markup, html};

use super::form::Fields;
use super::view::View;

const CHOICES: [&str; 3] = ["everyone", "groups", "students"];

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
