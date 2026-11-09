//! Confirmation in proportion to reach (#120). A `full` preview that makes
//! N Students left gets a danger-styled confirm naming N ("ยืนยัน: N คน
//! จะพ้นสภาพ"); when the preview also carries the shrink warning (more
//! than 20 % left, or a file shorter than the roster) Staff must type N
//! first: only the number, never a word such as "ยืนยัน" (#135). The
//! confirm handler checks the typed text here, on the server, through
//! [`crate::ui::typed_count_ok`]: the form only shows the field.

use maud::{Markup, html};

use super::store::Import;
use super::view::{BASE, View};

/// The form field holding the typed confirmation.
pub(super) const FIELD: &str = "typed";

/// How many Students confirming `imp` makes left: 0 for `upsert`.
pub(super) fn leaving(imp: &Import) -> u64 {
    if imp.mode == "full" {
        imp.counts.preview.will_become_left
    } else {
        0
    }
}

/// Whether confirming `imp` needs the typed confirmation: a `full`
/// import that makes someone left and carries the shrink warning.
pub(super) fn needs_typed(imp: &Import) -> bool {
    leaving(imp) > 0 && imp.counts.preview.warning
}

/// Whether `typed` confirms `imp`: not needed, or the number of Students
/// who will become left (ASCII or Thai digits, thousands separators and
/// spaces ignored). A word ("ยืนยัน", "confirm") no longer does (#135).
pub(super) fn typed_ok(imp: &Import, typed: Option<&str>) -> bool {
    !needs_typed(imp) || crate::ui::typed_count_ok(typed, leaving(imp))
}

/// The id of the confirm form, which the typed field joins with `form=`.
const FORM_ID: &str = "roster-confirm";

/// The typed field a warned `full` preview needs (nothing otherwise),
/// shown above the buttons; `wrong`: the last try did not match.
pub(super) fn typed_field(v: &View<'_>, imp: &Import, wrong: bool) -> Markup {
    if !needs_typed(imp) {
        return html! {};
    }
    let n = leaving(imp).to_string();
    html! {
        div class="field confirm-reach" {
            label for="roster-typed" { (v.t("kernel.roster.typed_label").replace("{n}", &n)) }
            input id="roster-typed" type="text" name=(FIELD) form=(FORM_ID) required
                data-reach=(n)
                autocomplete="off" spellcheck="false" enterkeyhint="done"
                aria-invalid=[wrong.then_some("true")]
                aria-describedby=[wrong.then_some("roster-typed-error")];
            @if wrong {
                p id="roster-typed-error" class="field-error" {
                    (v.t("kernel.roster.typed_wrong").replace("{n}", &n))
                }
            }
        }
    }
}

/// The confirm form of a preview: danger-styled and naming the reach when
/// someone will become left, the plain confirm otherwise.
pub(super) fn confirm_form(v: &View<'_>, imp: &Import) -> Markup {
    let n = leaving(imp);
    html! {
        form id=(FORM_ID) method="post" action=(format!("{BASE}/{}/confirm", imp.id)) {
            (v.staff.csrf_field())
            @if n > 0 {
                (crate::ui::danger_button(
                    &v.t("kernel.roster.confirm_left").replace("{n}", &n.to_string())
                ))
            } @else {
                button type="submit" { (v.t("kernel.roster.confirm")) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imp(mode: &str, left: u64, warning: bool) -> Import {
        let mut i = Import {
            mode: mode.into(),
            ..Import::default()
        };
        i.counts.preview.will_become_left = left;
        i.counts.preview.warning = warning;
        i
    }

    #[test]
    fn only_a_warned_full_import_that_makes_someone_left_asks_for_typing() {
        assert!(needs_typed(&imp("full", 3, true)));
        assert!(!needs_typed(&imp("full", 3, false)));
        assert!(!needs_typed(&imp("full", 0, true)));
        assert!(!needs_typed(&imp("upsert", 3, true)));
        assert_eq!(leaving(&imp("upsert", 3, true)), 0);
    }

    #[test]
    fn only_the_number_confirms_and_a_word_does_not() {
        let i = imp("full", 1234, true);
        for ok in ["1234", " 1,234 ", "๑๒๓๔"] {
            assert!(typed_ok(&i, Some(ok)), "{ok}");
        }
        let bad = [
            "",
            "  ",
            "1233",
            "12345",
            "-1234",
            "ยืนยัน",
            "Confirm",
            "yes",
            "1234x",
        ];
        for bad in bad {
            assert!(!typed_ok(&i, Some(bad)), "{bad}");
        }
        assert!(!typed_ok(&i, None));
        assert!(typed_ok(&imp("full", 3, false), None), "not needed");
    }
}
