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
//!   them (and deleted again when the batch fails or loses its race); the
//!   objects of files an edit removed or replaced are deleted after it.

use super::files_sniff::ImageType;
use super::gate::GATED;
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
/// Largest create / edit request: every image at its largest plus the
/// form (the preview's 128 KB), with room for the multipart framing.
pub(crate) const FORM_BODY_LIMIT: usize = MAX_IMAGES as usize * MAX_IMAGE_BYTES + 256 * 1024;
/// The public route's prefix.
pub(crate) const PUBLIC_PREFIX: &str = "/announcements/files";
/// Bytes of a token (128 bits).
const TOKEN_BYTES: usize = 16;

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

/// The files of announcement `id` of `kind`, by slot.
pub(crate) async fn list(s: &Services, id: &str, kind: &str) -> Result<Vec<Stored>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM announcement_file f WHERE f.announcement_id = ? AND f.kind = ? \
         ORDER BY f.n"
    );
    let rows = s.db.query(Statement::new(sql).bind(id).bind(kind)).await?;
    Ok((0..rows.len()).map(|i| stored_at(&rows, i)).collect())
}

/// The image in slot `n` of `images`.
pub(crate) fn slot(images: &[Stored], n: u8) -> Option<&Stored> {
    images.iter().find(|f| f.n == n)
}

/// The image a chat Card shows (#109): the first one `body` places that
/// exists, else the lowest slot.
pub(crate) fn hero<'a>(images: &'a [Stored], body: &str) -> Option<&'a Stored> {
    let placed = crate::rich_text::parse(body).images();
    placed
        .iter()
        .find_map(|n| slot(images, *n))
        .or_else(|| images.first())
}

/// The images `body` does not place, by slot: the Mini App shows them
/// after the body.
pub(crate) fn unplaced<'a>(images: &'a [Stored], body: &str) -> Vec<&'a Stored> {
    let placed = crate::rich_text::parse(body).images();
    images.iter().filter(|f| !placed.contains(&f.n)).collect()
}

/// A new image from the form, already sniffed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewImage {
    pub(crate) n: u8,
    pub(crate) kind: ImageType,
    pub(crate) size: Option<(u32, u32)>,
    pub(crate) bytes: Vec<u8>,
}

/// What a save does to the images: slots to fill (replacing any image
/// there) and slots to empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ImageEdits {
    pub(crate) put: Vec<NewImage>,
    pub(crate) remove: Vec<u8>,
}

impl ImageEdits {
    pub(crate) fn is_empty(&self) -> bool {
        self.put.is_empty() && self.remove.is_empty()
    }

    /// The audit summary: slots only, never bytes.
    pub(crate) fn summary(&self) -> serde_json::Value {
        let put: Vec<u8> = self.put.iter().map(|p| p.n).collect();
        serde_json::json!({ "put": put, "remove": self.remove })
    }
}

/// A save's images, staged: the objects already put, the statements that
/// list them, and the objects to delete once the save committed.
#[derive(Debug, Default)]
pub(crate) struct Staged {
    rows: Vec<(NewImage, String, String)>,
    emptied: Vec<u8>,
    /// Objects of the files this save removes or replaces.
    pub(crate) old_keys: Vec<String>,
}

/// Puts the objects of `edits` for announcement `id`, whose images are
/// `existing` now.
pub(crate) async fn stage(
    s: &Services,
    id: &str,
    edits: &ImageEdits,
    existing: &[Stored],
) -> Result<Staged> {
    let mut staged = Staged::default();
    for n in edits.remove.iter().chain(edits.put.iter().map(|p| &p.n)) {
        if let Some(old) = slot(existing, *n)
            && !staged.emptied.contains(n)
        {
            staged.old_keys.push(old.file_key.clone());
        }
        if !staged.emptied.contains(n) {
            staged.emptied.push(*n);
        }
    }
    for img in &edits.put {
        let token = s.random_hex(TOKEN_BYTES)?;
        let key = format!("announcement-file/{id}/{token}.{}", img.kind.ext());
        if let Err(e) = s
            .files
            .put(&key, img.bytes.clone(), img.kind.content_type())
            .await
        {
            staged.discard(s).await;
            return Err(e);
        }
        staged.rows.push((img.clone(), token, key));
    }
    Ok(staged)
}

impl Staged {
    /// The statements: empty every slot touched, then list the new files;
    /// each gated on `gate` when given (an edit's compare-and-set).
    pub(crate) fn statements(&self, id: &str, gate: Option<&str>, now: i64) -> Vec<Statement> {
        let cond = |sql: String| match gate {
            Some(_) => format!("{sql} AND {GATED}"),
            None => sql,
        };
        let with_gate = |st: Statement| match gate {
            Some(g) => st.bind(g),
            None => st,
        };
        let mut out = Vec::new();
        for n in &self.emptied {
            let sql = cond(
                "DELETE FROM announcement_file WHERE announcement_id = ? AND kind = 'image' \
                 AND n = ?"
                    .into(),
            );
            out.push(with_gate(Statement::new(sql).bind(id).bind(i64::from(*n))));
        }
        for (img, token, key) in &self.rows {
            let sql = cond(
                "INSERT INTO announcement_file (announcement_id, kind, n, content_type, \
                 size_bytes, width, height, file_name, token, file_key, created_at) \
                 SELECT ?, 'image', ?, ?, ?, ?, ?, ?, ?, ?, ? WHERE 1"
                    .into(),
            );
            let (w, h) = img.size.unzip();
            let st = Statement::new(sql)
                .bind(id)
                .bind(i64::from(img.n))
                .bind(img.kind.content_type())
                .bind(img.bytes.len() as i64)
                .bind(w.map(i64::from))
                .bind(h.map(i64::from))
                .bind(format!("image-{}.{}", img.n, img.kind.ext()))
                .bind(token.as_str())
                .bind(key.as_str())
                .bind(now);
            out.push(with_gate(st));
        }
        out
    }

    /// The save failed: delete the objects put for it.
    pub(crate) async fn discard(&self, s: &Services) {
        for (_, _, key) in &self.rows {
            // Unlisted, so never served; a failed delete leaves bytes only.
            let _ = s.files.delete(key).await;
        }
    }

    /// The save committed: delete the objects it removed or replaced.
    pub(crate) async fn finish(&self, s: &Services) {
        for key in &self.old_keys {
            // The row is gone, so the object is never served again; a
            // failed delete leaves bytes only.
            let _ = s.files.delete(key).await;
        }
    }
}

/// `body` as the Mini App shows it (#109): each placed image in place,
/// then the images it does not place, alt `"{label} N"`. `staff` serves
/// them through the back office (any status), else the public path.
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
    }
}
