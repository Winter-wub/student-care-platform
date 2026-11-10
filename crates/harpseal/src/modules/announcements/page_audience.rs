//! The Audience fieldset of the create / edit form (#140, moved from
//! [`super::page_form`] so saved Audiences, #144, and bulk announce, #145,
//! have one place to build on): the choice, the attribute filters, Tags,
//! the hand-picked list, the unreachable box and the live count; saved
//! Audiences (#144, [`page_saved`]) pick at the top and save at the bottom.

use maud::{Markup, html};

use super::form::Fields;
use super::form_scope;
use super::view::View;
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
    html! { (page_saved::page_parts()) (fieldset_noted(v, f, errors, note)) }
}

/// The fieldset alone, with `note` under its legend: what an HTMX answer
/// swaps in. The saved Audiences picker sits at the top, the save box at
/// the bottom ([`page_saved`]).
pub(super) fn fieldset_noted(
    v: &View<'_>,
    f: &Fields,
    errors: &[(&'static str, &'static str)],
    note: Markup,
) -> Markup {
    html! {
        fieldset id="audience" aria-describedby=[form_scope::described(errors, "audience")] {
            legend { (v.t("announcements.form.audience")) }
            (form_scope::field_errors(v, errors, "audience"))
            (note)
            (page_saved::picker_slot())
            (form_scope::field(v, f))
            p class="muted" { (v.t("announcements.form.audience_help")) }
            (form_scope::attributes(v, f))
            (page_saved::stale(v, f))
            div class="field" {
                label for="tags" { (v.t("announcements.form.tags")) }
                input id="tags" name="tags" value=(f.tags) placeholder=(v.t("announcements.form.any_of"));
            }
            (page_pick::field(v, f))
            label class="check" {
                input type="checkbox" name="unreachable" value="1" checked[f.unreachable];
                " " (v.t("announcements.form.include_unreachable"))
            }
            (page_count::placeholder(v))
            (page_saved::save_field(v, "", None))
        }
    }
}
