//! The optional parts of the create / edit form behind disclosures (#140):
//! the English version, images, PDFs, and the expiry with the
//! acknowledgement. Title, body, Audience and the send choice stay in
//! view; a part opens on load when it holds a value or an error, so
//! nothing Staff wrote or must fix is hidden. Closed, a part's fields are
//! still in the form and still posted: only the drawing changes.

use maud::{Markup, html};

use super::files::{IMAGE, PDF, Stored};
use super::form::Fields;

/// Error fields of each part, as [`super::form::Fields::draft`] and the
/// file checks name them.
const ENGLISH: &[&str] = &["en"];
const IMAGES: &[&str] = &["images", "image1", "image2", "image3", "image4"];
const PDFS: &[&str] = &["pdfs", "pdf1", "pdf2", "pdf3"];
const MORE: &[&str] = &["expires"];

/// Which optional parts open on load.
pub(super) struct Open {
    pub(super) english: bool,
    pub(super) images: bool,
    pub(super) pdfs: bool,
    pub(super) more: bool,
}

impl Open {
    /// A part opens when it holds a value (typed, saved or ticked) or an
    /// error; the images also open when chosen files were lost (#109).
    pub(super) fn of(
        f: &Fields,
        files: &[Stored],
        errors: &[(&str, &str)],
        reselect: bool,
    ) -> Self {
        let erred = |fields: &[&str]| errors.iter().any(|(e, _)| fields.contains(e));
        let saved = |kind: &str| files.iter().any(|s| s.kind == kind);
        let typed = |s: &str| !s.trim().is_empty();
        Self {
            english: typed(&f.title_en) || typed(&f.body_en) || erred(ENGLISH),
            images: saved(IMAGE) || reselect || erred(IMAGES),
            pdfs: saved(PDF) || erred(PDFS),
            more: typed(&f.expires) || f.require_ack || erred(MORE),
        }
    }
}

/// `body` behind a disclosure titled `summary`, open when `open`. A
/// fieldset inside keeps its legend for screen readers; the summary is the
/// visible name, so the legend is not drawn twice.
pub(super) fn part(id: &str, summary: &str, open: bool, body: Markup) -> Markup {
    html! {
        details class="optional" id=(id) open[open] {
            summary { (summary) }
            div class="optional-body" { (body) }
        }
    }
}
