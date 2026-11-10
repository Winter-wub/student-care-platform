//! The optional parts of the create / edit form behind disclosures (#140):
//! the English version, images, PDFs, and the expiry with the
//! acknowledgement. Title, body, Audience and the send choice stay in
//! view; a part opens on load when it holds a value or an error, so
//! nothing Staff wrote or must fix is hidden. Closed, a part's fields are
//! still in the form and still posted: only the drawing changes.

use maud::{Markup, html};

use super::files::{IMAGE, PDF, Stored};
use super::form::Fields;
use super::form_images::{self, Chosen};
use super::form_pdfs;

/// Which optional parts open on load.
pub(super) struct Open {
    pub(super) english: bool,
    pub(super) images: bool,
    pub(super) pdfs: bool,
    pub(super) more: bool,
}

impl Open {
    /// A part opens when it holds a value (typed, saved or ticked) or an
    /// error; the images and the PDFs each open when a file of their kind
    /// was chosen and lost (#109). The image and PDF error fields are the
    /// ones their own modules name ([`form_images::erred_in`],
    /// [`form_pdfs::erred_in`]), so the two cannot drift apart.
    pub(super) fn of(f: &Fields, files: &[Stored], errors: &[(&str, &str)], lost: Chosen) -> Self {
        let erred = |is: fn(&str) -> bool| errors.iter().any(|(e, _)| is(e));
        let saved = |kind: &str| files.iter().any(|s| s.kind == kind);
        let typed = |s: &str| !s.trim().is_empty();
        Self {
            english: typed(&f.title_en) || typed(&f.body_en) || erred(|e| e == "en"),
            images: saved(IMAGE) || lost.images || erred(form_images::erred_in),
            pdfs: saved(PDF) || lost.pdfs || erred(form_pdfs::erred_in),
            more: typed(&f.expires) || f.require_ack || erred(|e| e == "expires"),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn open(errors: &[(&str, &str)], lost: Chosen) -> Open {
        Open::of(&Fields::default(), &[], errors, lost)
    }

    #[test]
    fn each_file_kind_opens_only_its_own_part() {
        let pdf = open(
            &[],
            Chosen {
                images: false,
                pdfs: true,
            },
        );
        assert!(pdf.pdfs && !pdf.images);
        let img = open(
            &[],
            Chosen {
                images: true,
                pdfs: false,
            },
        );
        assert!(img.images && !img.pdfs);
        let none = open(&[], Chosen::default());
        assert!(!none.images && !none.pdfs && !none.english && !none.more);
    }

    #[test]
    fn every_image_and_pdf_error_field_opens_its_part() {
        for f in ["images", "image1", "image2", "image3", "image4"] {
            let o = open(&[(f, "k")], Chosen::default());
            assert!(o.images && !o.pdfs, "{f}");
        }
        for f in ["pdfs", "pdf1", "pdf2", "pdf3"] {
            let o = open(&[(f, "k")], Chosen::default());
            assert!(o.pdfs && !o.images, "{f}");
        }
        assert!(open(&[("en", "k")], Chosen::default()).english);
        assert!(open(&[("expires", "k")], Chosen::default()).more);
    }
}
