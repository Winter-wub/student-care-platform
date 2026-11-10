//! The Audience fieldset of the create / edit form (#140, moved from
//! [`super::page_form`] so saved Audiences, #144, and bulk announce, #145,
//! have one place to build on): the choice, the attribute filters, Tags,
//! the hand-picked list, the unreachable box and the live count; saved
//! Audiences (#144, [`page_saved`]) pick at the top and save at the bottom.
//!
//! Each choice shows only its own fields (#156): the attribute filters and
//! Tags under "เลือกกลุ่ม", the hand-picked list under "เลือกรายคน"
//! ([`form_scope::SCRIPT`]); without JS all of them show.

use maud::{Markup, PreEscaped, html};

use super::form::Fields;
use super::view::View;
use super::{form_attrs, form_scope};
use super::{page_count, page_pick, page_saved};

/// The Audience fieldset (`#audience`). Its fields are [`Fields`]'
/// Audience fields, so a caller that prefills them (a saved Audience, a
/// bulk announce) gets them drawn here.
pub(super) fn fieldset(
    v: &View<'_>,
    f: &Fields,
    errors: &[(&'static str, &'static str)],
) -> Markup {
    fieldset_with_note(v, f, errors, html! {})
}

/// [`fieldset`] with `note` under its legend (#144: which saved Audience
/// filled it), after the saved Audiences' live region and script
/// ([`page_saved::page_parts`]). For a whole page.
pub(super) fn fieldset_with_note(
    v: &View<'_>,
    f: &Fields,
    errors: &[(&'static str, &'static str)],
    note: Markup,
) -> Markup {
    html! {
        (page_saved::page_parts())
        (page_count::said_region())
        script { (PreEscaped(form_scope::SCRIPT)) }
        (fieldset_noted(v, f, errors, note, false))
    }
}

/// The fieldset alone, with `note` under its legend: what an HTMX answer
/// swaps in. The saved Audiences picker sits at the top, the save box at
/// the bottom ([`page_saved`]). `scripted` (an HTMX answer): the parts of
/// the other choices start hidden, as the script would hide them.
pub(super) fn fieldset_noted(
    v: &View<'_>,
    f: &Fields,
    errors: &[(&'static str, &'static str)],
    note: Markup,
    scripted: bool,
) -> Markup {
    let hide = |part: &str| form_scope::hidden(f, part, scripted);
    html! {
        fieldset id="audience" aria-describedby=[form_scope::described(errors, "audience")] {
            legend { (v.t("announcements.form.audience")) }
            (form_scope::field_errors(v, errors, "audience"))
            (note)
            (page_saved::picker_slot())
            (form_scope::field(v, f))
            div class="scope-part" data-scope-for="groups" hidden[hide("groups")] {
                p class="muted" { (v.t("announcements.form.audience_help")) }
                (form_attrs::attributes(v, f))
                (page_saved::stale(v, f))
                div class="field" {
                    label for="tags" { (v.t("announcements.form.tags")) }
                    input id="tags" name="tags" value=(f.tags) placeholder=(v.t("announcements.form.any_of"));
                }
            }
            div class="scope-part" data-scope-for="students" hidden[hide("students")] {
                (page_pick::field(v, f))
            }
            label class="check" {
                input type="checkbox" name="unreachable" value="1" checked[f.unreachable];
                " " (v.t("announcements.form.include_unreachable"))
            }
            (page_count::placeholder(v))
            (page_saved::save_field(v, "", None))
        }
    }
}
