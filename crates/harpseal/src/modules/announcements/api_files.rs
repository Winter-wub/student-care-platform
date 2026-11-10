//! Files attached from code (#164): images and PDFs for
//! [`super::create_draft`], checked exactly as the create form checks its
//! uploads ([`super::form_images`], [`super::form_pdfs`]).
//!
//! - **Images**: slots 1 to 4, PNG or JPEG told by the bytes and named as
//!   what they are (`.png`, `.jpg`, `.jpeg`), at most 1 MB and 4096 pixels
//!   a side (16 megapixels).
//! - **PDFs**: slots 1 to 3, named `.pdf`, at most 10 MB each, refused when
//!   the scan finds active content. A form save takes one new PDF; code
//!   may fill every slot in one call (it holds the bytes already).
//! - **Type**: `content_type` must name what the bytes are
//!   (`image/png`, `image/jpeg`, `application/pdf`), else refused.
//! - **Alt text** lives in the body, as on the form: `![alt](image:N)`
//!   places image `N` with that alt; an image the body does not place is
//!   shown after it with the alt `"<label> N"`, the label the page
//!   passes (`รูปภาพ` on the Kernel's Thai pages). There is no alt field.

use super::files::FileEdits;
use super::form_images::{FilePart, Posted};

/// What a file attached from code is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileKind {
    /// A PNG or JPEG image (slots 1 to 4).
    Image,
    /// A PDF (slots 1 to 3).
    Pdf,
}

/// An image or PDF for [`super::NewAnnouncement::files`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AnnouncementFile {
    /// Image or PDF.
    pub kind: FileKind,
    /// The slot: 1 to 4 for an image, 1 to 3 for a PDF. Each slot once.
    pub n: u8,
    /// The file name (its extension must match the bytes). A PDF is
    /// stored and served under it, made safe; an image as `image-N.ext`.
    pub name: String,
    /// The MIME type; must agree with the bytes.
    pub content_type: String,
    /// The file.
    pub bytes: Vec<u8>,
}

impl AnnouncementFile {
    /// Image `n` (1 to 4).
    pub fn image(
        n: u8,
        name: impl Into<String>,
        content_type: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Self {
        Self::new(FileKind::Image, n, name, content_type, bytes)
    }

    /// PDF `n` (1 to 3).
    pub fn pdf(
        n: u8,
        name: impl Into<String>,
        content_type: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Self {
        Self::new(FileKind::Pdf, n, name, content_type, bytes)
    }

    fn new(
        kind: FileKind,
        n: u8,
        name: impl Into<String>,
        content_type: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            kind,
            n,
            name: name.into(),
            content_type: content_type.into(),
            bytes,
        }
    }

    /// The form field this file would have been posted in.
    fn field(&self) -> String {
        match self.kind {
            FileKind::Image => format!("image{}", self.n),
            FileKind::Pdf => format!("pdf{}", self.n),
        }
    }
}

type Errors = Vec<(&'static str, &'static str)>;

/// The file edits of `files`, through the form's own checks, or every
/// problem as (field, catalog key).
pub(super) fn edits(files: &[AnnouncementFile]) -> Result<FileEdits, Errors> {
    let mut posted = Posted {
        pairs: Vec::new(),
        files: files
            .iter()
            .map(|f| FilePart {
                field: f.field(),
                file_name: f.name.clone(),
                bytes: f.bytes.clone(),
            })
            .collect(),
    };
    let edits = posted.file_edits_up_to(usize::from(super::files::MAX_PDFS))?;
    let mut errors = Vec::new();
    for put in &edits.put {
        let given = files
            .iter()
            .find(|f| f.n == put.n && (f.kind == FileKind::Pdf) == (put.kind == super::files::PDF));
        let agrees =
            given.is_some_and(|f| f.content_type.trim().eq_ignore_ascii_case(put.content_type));
        if !agrees {
            let key = if put.kind == super::files::PDF {
                "announcements.pdfs.bad_type"
            } else {
                "announcements.images.bad_type"
            };
            errors.push(("files", key));
        }
    }
    if errors.is_empty() {
        Ok(edits)
    } else {
        Err(super::form::unique(errors))
    }
}
