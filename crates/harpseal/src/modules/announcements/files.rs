//! Announcement files (#109): what Staff attach to an announcement, kept
//! in the FileStore and listed in `announcement_file` (Module migration
//! `0003_files`). Images are the first kind; PDFs (#113) reuse the table,
//! the token and the serving rules ([`super::files_serve`]).
//!
//! - **Key**: `announcement-file/<announcement id>/<token>.<ext>`, so a
//!   replaced file never overwrites the object of the one it replaces.
//! - **Token**: 16 bytes from the Random port as 32 hex characters, the
//!   public path `/announcements/files/<token>`.
//! - **Writes**: objects are put before the database batch that lists
//!   them. After the batch, whatever it reported, the keys it touched are
//!   re-read and only the objects no row lists are deleted
//!   ([`Staged::settle`]): a batch that committed but answered an error
//!   keeps its files.
//! - **Edits**: before sending (draft, scheduled) a removed or replaced
//!   file's row and object are deleted. Once sent, Cards already delivered
//!   show it: its row is marked `superseded_at` and it stays served (same
//!   token, same live check) until the announcement is retracted or
//!   expires. Pages and new Cards use the current files only.
//! - **PDFs** (#113): kind `pdf`, slots 1 to [`MAX_PDFS`], at most
//!   [`MAX_PDF_BYTES`] each, sniffed by [`super::files_pdf`], stored under
//!   their own name (made safe) and served with it.
//! - **Purge**: none yet. A retracted or expired announcement keeps its
//!   rows and objects, served to no one.

use std::collections::BTreeMap;

pub(crate) use super::files_stage::stage;
use super::store::{int, text};
use crate::port_types::Statement;
use crate::rich_text::Img;
use crate::{Result, Services};

/// The `kind` of an image.
pub(crate) const IMAGE: &str = "image";
/// Most images per announcement (slots 1 to 4).
pub(crate) const MAX_IMAGES: u8 = 4;
/// Largest image, in bytes (1 MB).
pub(crate) const MAX_IMAGE_BYTES: usize = 1024 * 1024;
/// The `kind` of a PDF (#113).
pub(crate) const PDF: &str = "pdf";
/// Most PDFs per announcement (slots 1 to 3).
pub(crate) const MAX_PDFS: u8 = 3;
/// Largest PDF, in bytes (10 MB).
pub(crate) const MAX_PDF_BYTES: usize = 10 * 1024 * 1024;
/// New PDFs one save takes (#113 review): the others are added by saving
/// again, so a request never holds more than one.
pub(crate) const PDFS_PER_SAVE: usize = 1;
/// Largest create / edit request: 15,204,352 bytes (14.5 MiB). Every
/// image at its largest (4 MiB), one PDF at its largest (10 MiB) and
/// 512 KiB for the text fields and the multipart framing. A Worker holds
/// a request up to about four times (the CSRF read, the multipart buffer,
/// the file parts, the copy handed to R2), so one save peaks near 58 MiB of
/// a 128 MB isolate; with three PDFs per save it was 137 MiB.
pub(crate) const FORM_BODY_LIMIT: usize =
    MAX_IMAGES as usize * MAX_IMAGE_BYTES + PDFS_PER_SAVE * MAX_PDF_BYTES + 512 * 1024;
/// The public route's prefix.
pub(crate) const PUBLIC_PREFIX: &str = "/announcements/files";

/// One stored file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stored {
    pub(crate) announcement_id: String,
    pub(crate) kind: String,
    pub(crate) n: u8,
    pub(crate) content_type: String,
    pub(crate) size_bytes: i64,
    pub(crate) size: Option<(u32, u32)>,
    pub(crate) file_name: String,
    pub(crate) token: String,
    pub(crate) file_key: String,
}

impl Stored {
    /// The public path (no origin).
    pub(crate) fn public_path(&self) -> String {
        format!("{PUBLIC_PREFIX}/{}", self.token)
    }

    /// The back-office path (`announcements.view`, any status).
    pub(crate) fn staff_path(&self) -> String {
        format!(
            "/admin/announcements/{}/files/{}",
            self.announcement_id, self.token
        )
    }

    /// The image for [`crate::rich_text::Doc::html_with`]: the public path
    /// for the Mini App, the back-office one for Staff.
    pub(crate) fn img(&self, staff: bool) -> Img {
        let src = if staff {
            self.staff_path()
        } else {
            self.public_path()
        };
        Img::new(src, self.size)
    }
}

pub(crate) const COLUMNS: &str = "f.announcement_id, f.kind, f.n, f.content_type, f.size_bytes, \
     f.width, f.height, f.file_name, f.token, f.file_key";

pub(crate) fn stored_at(rows: &crate::port_types::Rows, i: usize) -> Stored {
    let px = |c: &str| int(rows, i, c).and_then(|v| u32::try_from(v).ok());
    Stored {
        announcement_id: text(rows, i, "announcement_id"),
        kind: text(rows, i, "kind"),
        n: int(rows, i, "n")
            .and_then(|v| u8::try_from(v).ok())
            .unwrap_or(0),
        content_type: text(rows, i, "content_type"),
        size_bytes: int(rows, i, "size_bytes").unwrap_or(0),
        size: px("width").zip(px("height")),
        file_name: text(rows, i, "file_name"),
        token: text(rows, i, "token"),
        file_key: text(rows, i, "file_key"),
    }
}

/// The current files (not superseded) of announcement `id`, every kind
/// (images, then PDFs), by slot. The image helpers below look at images
/// only, so callers pass the whole list.
pub(crate) async fn list(s: &Services, id: &str) -> Result<Vec<Stored>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM announcement_file f WHERE f.announcement_id = ? \
         AND f.superseded_at IS NULL ORDER BY f.kind, f.n"
    );
    let rows = s.db.query(Statement::new(sql).bind(id)).await?;
    Ok((0..rows.len()).map(|i| stored_at(&rows, i)).collect())
}

/// [`list`] for many announcements in one query per 90 ids (D1 binds at
/// most 100), by announcement id.
pub(crate) async fn list_many(s: &Services, ids: &[&str]) -> Result<BTreeMap<String, Vec<Stored>>> {
    let mut out: BTreeMap<String, Vec<Stored>> = BTreeMap::new();
    for chunk in ids.chunks(90) {
        let marks = vec!["?"; chunk.len()].join(", ");
        let sql = format!(
            "SELECT {COLUMNS} FROM announcement_file f \
             WHERE f.superseded_at IS NULL AND f.announcement_id IN ({marks}) \
             ORDER BY f.announcement_id, f.kind, f.n"
        );
        let st = chunk
            .iter()
            .fold(Statement::new(sql), |st, id| st.bind(*id));
        let rows = s.db.query(st).await?;
        for i in 0..rows.len() {
            let f = stored_at(&rows, i);
            out.entry(f.announcement_id.clone()).or_default().push(f);
        }
    }
    Ok(out)
}

/// The image in slot `n` of `files`.
pub(crate) fn slot(files: &[Stored], n: u8) -> Option<&Stored> {
    files.iter().find(|f| f.kind == IMAGE && f.n == n)
}

/// The PDFs of `files`, by slot (#113).
pub(crate) fn pdfs(files: &[Stored]) -> impl Iterator<Item = &Stored> {
    files.iter().filter(|f| f.kind == PDF)
}

/// The PDF in slot `n` of `files`.
pub(crate) fn pdf_slot(files: &[Stored], n: u8) -> Option<&Stored> {
    pdfs(files).find(|f| f.n == n)
}

/// The image a chat Card shows (#109): the first one `body` places that
/// exists, else the lowest slot.
pub(crate) fn hero<'a>(images: &'a [Stored], body: &str) -> Option<&'a Stored> {
    let placed = crate::rich_text::parse(body).images();
    placed
        .iter()
        .find_map(|n| slot(images, *n))
        .or_else(|| images.iter().find(|f| f.kind == IMAGE))
}

/// The images `body` does not place, by slot: the Mini App shows them
/// after the body.
pub(crate) fn unplaced<'a>(images: &'a [Stored], body: &str) -> Vec<&'a Stored> {
    let placed = crate::rich_text::parse(body).images();
    images
        .iter()
        .filter(|f| f.kind == IMAGE && !placed.contains(&f.n))
        .collect()
}

/// A new file from the form, already checked: an image (sniffed, its
/// size known) or a PDF (#113, sniffed, its name made safe).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewFile {
    /// [`IMAGE`] or [`PDF`].
    pub(crate) kind: &'static str,
    pub(crate) n: u8,
    pub(crate) content_type: &'static str,
    /// The object key's extension.
    pub(crate) ext: &'static str,
    /// Pixels (images only).
    pub(crate) size: Option<(u32, u32)>,
    /// The name stored and served (`image-N.png`; a PDF's own name).
    pub(crate) name: String,
    pub(crate) bytes: Vec<u8>,
}

/// What a save does to the files: slots to fill (replacing any file
/// there) and (kind, slot) pairs to empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FileEdits {
    pub(crate) put: Vec<NewFile>,
    pub(crate) remove: Vec<(&'static str, u8)>,
}

impl FileEdits {
    pub(crate) fn is_empty(&self) -> bool {
        self.put.is_empty() && self.remove.is_empty()
    }

    /// The audit summary: slots only, never bytes nor names. Images as
    /// in #109 (`put`, `remove`); PDFs under `pdfs` when any.
    pub(crate) fn summary(&self) -> serde_json::Value {
        let put = |k: &str| -> Vec<u8> {
            self.put
                .iter()
                .filter(|p| p.kind == k)
                .map(|p| p.n)
                .collect()
        };
        let remove = |k: &str| -> Vec<u8> {
            self.remove
                .iter()
                .filter(|r| r.0 == k)
                .map(|r| r.1)
                .collect()
        };
        let mut j = serde_json::json!({ "put": put(IMAGE), "remove": remove(IMAGE) });
        let (pp, pr) = (put(PDF), remove(PDF));
        if !pp.is_empty() || !pr.is_empty() {
            j["pdfs"] = serde_json::json!({ "put": pp, "remove": pr });
        }
        j
    }
}

/// `body` as the Mini App shows it (#109): each placed image in place,
/// then the images it does not place, alt `"{label} N"`, then the PDFs
/// (#113) with their names and sizes. `staff` serves them through the back
/// office (any status), else the public path.
pub(crate) fn body_html(body: &str, images: &[Stored], staff: bool, label: &str) -> maud::Markup {
    let doc = crate::rich_text::parse(body);
    let rest = unplaced(images, body);
    maud::html! {
        (doc.html_with(&|n| slot(images, n).map(|f| f.img(staff))))
        @if !rest.is_empty() {
            div class="app-images" {
                @for f in rest { (f.img(staff).markup(&format!("{label} {}", f.n))) }
            }
        }
        (super::files_pdf::list_html(images, staff))
    }
}
