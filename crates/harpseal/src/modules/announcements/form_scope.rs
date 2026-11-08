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
