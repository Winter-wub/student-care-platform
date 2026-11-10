//! The Audience fieldset of the create / edit form (#140, moved from
//! [`super::page_form`] so saved Audiences, #144, and bulk announce, #145,
//! have one place to build on): the choice, the attribute filters, Tags,
//! the hand-picked list, the unreachable box and the live count.

use maud::{Markup, html};

use super::form::Fields;
use super::form_scope;
use super::view::View;
use super::{page_count, page_pick};

/// The Audience fieldset (`#audience`). Its fields are [`Fields`]'
/// Audience fields, so a caller that prefills them (a saved Audience, a
/// bulk announce) gets them drawn here.
pub(super) fn fieldset(
    v: &View<'_>,
    f: &Fields,
    errors: &[(&'static str, &'static str)],
) -> Markup {
    html! {
        fieldset id="audience" aria-describedby=[form_scope::described(errors, "audience")] {
            legend { (v.t("announcements.form.audience")) }
            (form_scope::field_errors(v, errors, "audience"))
            (form_scope::field(v, f))
            p class="muted" { (v.t("announcements.form.audience_help")) }
            (form_scope::attributes(v, f))
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
        }
    }
}
