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
//! - **Purge**: none yet. A retracted or expired announcement keeps its
//!   rows and objects, served to no one.

use std::collections::BTreeMap;

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

/// The current files (not superseded) of announcement `id` of `kind`, by
/// slot.
pub(crate) async fn list(s: &Services, id: &str, kind: &str) -> Result<Vec<Stored>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM announcement_file f WHERE f.announcement_id = ? AND f.kind = ? \
         AND f.superseded_at IS NULL ORDER BY f.n"
    );
    let rows = s.db.query(Statement::new(sql).bind(id).bind(kind)).await?;
    Ok((0..rows.len()).map(|i| stored_at(&rows, i)).collect())
}

/// [`list`] for many announcements in one query per 90 ids (D1 binds at
/// most 100), by announcement id.
pub(crate) async fn list_many(
    s: &Services,
    ids: &[&str],
    kind: &str,
) -> Result<BTreeMap<String, Vec<Stored>>> {
    let mut out: BTreeMap<String, Vec<Stored>> = BTreeMap::new();
    for chunk in ids.chunks(90) {
        let marks = vec!["?"; chunk.len()].join(", ");
        let sql = format!(
            "SELECT {COLUMNS} FROM announcement_file f WHERE f.kind = ? \
             AND f.superseded_at IS NULL AND f.announcement_id IN ({marks}) \
             ORDER BY f.announcement_id, f.n"
        );
        let st = chunk
            .iter()
            .fold(Statement::new(sql).bind(kind), |st, id| st.bind(*id));
        let rows = s.db.query(st).await?;
        for i in 0..rows.len() {
            let f = stored_at(&rows, i);
            out.entry(f.announcement_id.clone()).or_default().push(f);
        }
    }
    Ok(out)
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

/// A new image from the form, already sniffed (its size is always known:
/// the sniff refuses an image without one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewImage {
    pub(crate) n: u8,
    pub(crate) kind: ImageType,
    pub(crate) size: (u32, u32),
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

/// One new file, staged: its object is in the FileStore; the bytes are
/// not kept.
#[derive(Debug)]
struct NewRow {
    n: u8,
    kind: ImageType,
    size: (u32, u32),
    bytes: i64,
    token: String,
    key: String,
}

/// A save's images, staged: the objects already put, the statements that
/// list them, and the keys to settle once the save is over.
#[derive(Debug, Default)]
pub(crate) struct Staged {
    rows: Vec<NewRow>,
    emptied: Vec<u8>,
    /// Supersede instead of delete: the announcement was sent, and Cards
    /// already delivered show the old files.
    keep_old: bool,
    /// Objects of the files this save deletes (not sent yet only).
    old_keys: Vec<String>,
}

/// Puts the objects of `edits` (the bytes moved, not copied) for
/// announcement `id`, whose current images are `existing`. With `sent`,
/// the files removed or replaced stay listed and served, superseded
/// ([`Staged::statements`]).
pub(crate) async fn stage(
    s: &Services,
    id: &str,
    edits: ImageEdits,
    existing: &[Stored],
    sent: bool,
) -> Result<Staged> {
    let mut staged = Staged {
        keep_old: sent,
        ..Staged::default()
    };
    for n in edits.remove.iter().chain(edits.put.iter().map(|p| &p.n)) {
        if staged.emptied.contains(n) {
            continue;
        }
        staged.emptied.push(*n);
        if let Some(old) = slot(existing, *n).filter(|_| !sent) {
            staged.old_keys.push(old.file_key.clone());
        }
    }
    for img in edits.put {
        let token = s.random_hex(TOKEN_BYTES)?;
        let key = format!("announcement-file/{id}/{token}.{}", img.kind.ext());
        let bytes = img.bytes.len() as i64;
        if let Err(e) = s.files.put(&key, img.bytes, img.kind.content_type()).await {
            staged.settle(s).await;
            return Err(e);
        }
        staged.rows.push(NewRow {
            n: img.n,
            kind: img.kind,
            size: img.size,
            bytes,
            token,
            key,
        });
    }
    Ok(staged)
}

impl Staged {
    /// The statements: empty every slot touched (delete the row, or mark it
    /// superseded once sent), then list the new files; each gated on
    /// `gate` when given (an edit's compare-and-set).
    pub(crate) fn statements(&self, id: &str, gate: Option<&str>, now: i64) -> Vec<Statement> {
        let cond = |sql: &str| match gate {
            Some(_) => format!("{sql} AND {GATED}"),
            None => sql.to_string(),
        };
        let with_gate = |st: Statement| match gate {
            Some(g) => st.bind(g),
            None => st,
        };
        let empty = if self.keep_old {
            format!("UPDATE announcement_file SET superseded_at = {now} WHERE")
        } else {
            "DELETE FROM announcement_file WHERE".to_string()
        };
        let mut out = Vec::new();
        for n in &self.emptied {
            let sql = cond(&format!(
                "{empty} announcement_id = ? AND kind = 'image' AND n = ? AND superseded_at IS NULL"
            ));
            out.push(with_gate(Statement::new(sql).bind(id).bind(i64::from(*n))));
        }
        for r in &self.rows {
            let sql = cond(
                "INSERT INTO announcement_file (announcement_id, kind, n, content_type, \
                 size_bytes, width, height, file_name, token, file_key, created_at) \
                 SELECT ?, 'image', ?, ?, ?, ?, ?, ?, ?, ?, ? WHERE 1",
            );
            let st = Statement::new(sql)
                .bind(id)
                .bind(i64::from(r.n))
                .bind(r.kind.content_type())
                .bind(r.bytes)
                .bind(i64::from(r.size.0))
                .bind(i64::from(r.size.1))
                .bind(format!("image-{}.{}", r.n, r.kind.ext()))
                .bind(r.token.as_str())
                .bind(r.key.as_str())
                .bind(now);
            out.push(with_gate(st));
        }
        out
    }

    /// After the save, whatever its result (a batch may commit and still
    /// report an error): re-reads which of the keys it touched are listed,
    /// and deletes the objects of those that are not (new files of a save
    /// that did not commit, files a committed save deleted). When the
    /// re-read fails, nothing is deleted: an unlisted object is never
    /// served, a deleted listed one would be lost.
    pub(crate) async fn settle(&self, s: &Services) {
        let keys: Vec<&str> = self
            .rows
            .iter()
            .map(|r| r.key.as_str())
            .chain(self.old_keys.iter().map(String::as_str))
            .collect();
        for chunk in keys.chunks(90) {
            let marks = vec!["?"; chunk.len()].join(", ");
            let sql = format!("SELECT file_key FROM announcement_file WHERE file_key IN ({marks})");
            let st = chunk.iter().fold(Statement::new(sql), |st, k| st.bind(*k));
            let Ok(rows) = s.db.query(st).await else {
                return;
            };
            let listed: Vec<String> = (0..rows.len())
                .map(|i| text(&rows, i, "file_key"))
                .collect();
            for key in chunk.iter().filter(|k| !listed.iter().any(|l| l == *k)) {
                // A failed delete leaves bytes only: the key is unlisted.
                let _ = s.files.delete(key).await;
            }
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
