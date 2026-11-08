//! The explicit Audience choice on the form (#120): ทุกคน / เลือกกลุ่ม /
//! เลือกรายคน, none preselected on a new announcement. Without JS every
//! filter stays visible; the server reads the choice
//! ([`super::form::Fields::scope_problem`]).

use maud::{Markup, html};

use super::form::Fields;
use super::view::View;

const CHOICES: [&str; 3] = ["everyone", "groups", "students"];

pub(super) fn field(v: &View<'_>, f: &Fields) -> Markup {
    html! {
        div class="field scope" role="radiogroup" aria-label=(v.t("announcements.form.audience")) {
            @for c in CHOICES {
                label class="check" {
                    input type="radio" name="scope" value=(c) required checked[f.scope == c];
                    " " (v.t(&format!("announcements.form.scope_{c}")))
                }
            }
        }
    }
}
