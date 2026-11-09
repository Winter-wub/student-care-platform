//! The images part of the create / edit form (#109): the posted form
//! ([`Posted`], `multipart/form-data` from a browser, or a plain form
//! without files), the slots with the Thai hint, and the checks that turn
//! the posted files into [`FileEdits`] (the PDF slots are
//! [`super::form_pdfs`]).
//!
//! Slots `image1` to `image4`: a file chosen fills (or replaces) the
//! slot, the box `imageN_remove` empties it. Each file is sniffed
//! ([`super::files_sniff`]) and at most [`MAX_IMAGE_BYTES`]; the request as
//! a whole is capped before it is read (the route's `DefaultBodyLimit`
//! and the guard, [`super::files::FORM_BODY_LIMIT`]).

use axum::Form;
use axum::extract::{FromRequest, Multipart, Request};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};

use super::files::{FileEdits, IMAGE, MAX_IMAGE_BYTES, MAX_IMAGES, NewFile, Stored, slot};
use super::files_sniff::{HERO_SIDE, size_ok, sniff};
use super::view::View;
use crate::manifest::CatalogEntry as E;

/// The slot fields, `image1` to `image4` (also the error fields).
const SLOTS: [&str; MAX_IMAGES as usize] = ["image1", "image2", "image3", "image4"];

/// `hx-params` of the form's HTMX fragments (the Audience count, the
/// preview, the picker): never the files, which those routes neither need
/// nor take (their body limits are small).
pub(super) const NOT_FILES: &str = "not image1,image2,image3,image4,pdf1,pdf2,pdf3";

/// Image texts, Thai and English.
pub(crate) const TEXTS: &[E] = &[
    E::new("announcements.images.legend", "th", "รูปภาพ"),
    E::new("announcements.images.legend", "en", "Images"),
    E::new(
        "announcements.images.hint",
        "th",
        "ไม่เกิน 4 รูป รูปละไม่เกิน 1 MB (JPEG/PNG) ย่อรูปก่อนอัปโหลด \
         และห้ามใส่ข้อมูลส่วนตัวในรูป เพราะใครมีลิงก์ก็เปิดรูปได้",
    ),
    E::new(
        "announcements.images.hint",
        "en",
        "Up to 4 images, 1 MB each (JPEG/PNG). Shrink large images first, \
         and never put personal data in an image: anyone with its link can open it.",
    ),
    E::new(
        "announcements.images.place",
        "th",
        "พิมพ์ ![คำอธิบายรูป](image:1) ในเนื้อหา (ปุ่ม 🖼) เพื่อวางรูปตรงนั้น \
         รูปที่ไม่ได้วางจะแสดงต่อท้ายเนื้อหาในมินิแอป และรูปแรกจะอยู่บนการ์ดในแชท",
    ),
    E::new(
        "announcements.images.place",
        "en",
        "Type ![description](image:1) in a body (the 🖼 button) to place an image there. \
         Images not placed show after the body in the Mini App; the first is on the chat Card.",
    ),
    E::new(
        "announcements.images.no_public_url",
        "th",
        "ตอนนี้รูปจะแสดงในมินิแอปเท่านั้น การ์ดในแชทจะยังไม่มีรูป เพราะระบบยังไม่ได้ตั้งที่อยู่เว็บสาธารณะ (แจ้งผู้ดูแลระบบให้ตั้ง public_url)",
    ),
    E::new(
        "announcements.images.no_public_url",
        "en",
        "public_url is not set: images show in the Mini App only, not on chat Cards.",
    ),
    E::new("announcements.images.slot", "th", "รูปที่"),
    E::new("announcements.images.slot", "en", "Image"),
    E::new("announcements.images.remove", "th", "ลบรูปนี้"),
    E::new("announcements.images.remove", "en", "Remove this image"),
    E::new(
        "announcements.images.remove_sent",
        "th",
        "ประกาศนี้ส่งแล้ว: รูปที่ลบจะหายจากหน้าและการ์ดใหม่ แต่ยังเปิดได้จากลิงก์เดิม\
         ในการ์ดที่ส่งไปแล้ว จนกว่าจะถอนประกาศหรือประกาศหมดอายุ \
         ถ้ารูปมีข้อมูลส่วนตัว ให้ถอนประกาศ",
    ),
    E::new(
        "announcements.images.remove_sent",
        "en",
        "This announcement was sent: a removed image leaves the pages and new Cards, \
         but its old link in the Cards already delivered still opens it until the \
         announcement is retracted or expires. If it shows personal data, retract the announcement.",
    ),
    E::new(
        "announcements.images.bad_type",
        "th",
        "รับเฉพาะรูป JPEG หรือ PNG ที่นามสกุลไฟล์ตรงกับชนิดรูป (ไม่รับ SVG, GIF หรือไฟล์อื่น)",
    ),
    E::new(
        "announcements.images.bad_type",
        "en",
        "JPEG or PNG only, named as what it is (no SVG, GIF or other files).",
    ),
    E::new(
        "announcements.images.too_big",
        "th",
        "รูปใหญ่เกิน 1 MB ย่อรูปแล้วอัปโหลดใหม่",
    ),
    E::new(
        "announcements.images.too_big",
        "en",
        "Over 1 MB: shrink the image and upload it again.",
    ),
    E::new("announcements.images.too_many", "th", "แนบรูปได้ไม่เกิน 4 รูป"),
    E::new("announcements.images.too_many", "en", "At most 4 images."),
    E::new(
        "announcements.images.reselect",
        "th",
        "ไฟล์ที่เลือกไว้ยังไม่ได้บันทึก เลือกไฟล์อีกครั้ง",
    ),
    E::new(
        "announcements.images.reselect",
        "en",
        "The files chosen were not saved: choose them again.",
    ),
    E::new(
        "announcements.images.too_many_pixels",
        "th",
        "รูปใหญ่เกิน 4096 พิกเซลต่อด้าน หรือเกิน 16 ล้านพิกเซล ย่อรูปแล้วอัปโหลดใหม่",
    ),
    E::new(
        "announcements.images.too_many_pixels",
        "en",
        "Over 4096 pixels a side or 16 megapixels: shrink the image and upload it again.",
    ),
    E::new(
        "announcements.images.hero_large",
        "th",
        "รูปแรก (รูปบนการ์ดในแชท) ใหญ่เกิน 1024 พิกเซลต่อด้าน LINE อาจไม่แสดงรูปบนการ์ด \
         ควรย่อให้ไม่เกิน 1024×1024",
    ),
    E::new(
        "announcements.images.hero_large",
        "en",
        "The first image (on the chat Card) is over 1024 pixels a side: LINE may not show it. \
         Shrink it to 1024×1024 at most.",
    ),
    E::new("announcements.editor.image", "th", "รูปภาพ"),
    E::new("announcements.editor.image", "en", "Image"),
];

/// One file part as posted.
#[derive(Debug, Clone)]
pub(super) struct FilePart {
    pub(super) field: String,
    pub(super) file_name: String,
    /// The bytes, cut at one past the limit (enough to refuse it).
    pub(super) bytes: Vec<u8>,
}

/// The create / edit form as posted: its text fields and its files.
#[derive(Debug, Clone, Default)]
pub(super) struct Posted {
    pub(super) pairs: Vec<(String, String)>,
    pub(super) files: Vec<FilePart>,
}

impl Posted {
    /// Whether any file was chosen (lost when the form comes back); ask
    /// before [`Posted::file_edits`] moves them out.
    pub(super) fn had_files(&self) -> bool {
        !self.files.is_empty()
    }

    /// The image edits, the image files' bytes moved out (the PDF parts
    /// must be taken first: any other file is "too many"), or every
    /// problem as (field, catalog key), each once.
    pub(super) fn image_edits(&mut self) -> Result<FileEdits, Vec<(&'static str, &'static str)>> {
        let mut edits = FileEdits::default();
        let mut removed: Vec<u8> = Vec::new();
        let mut errors = Vec::new();
        for (k, _) in &self.pairs {
            if let Some(n) = k.strip_suffix("_remove").and_then(slot_of) {
                removed.push(n);
            }
        }
        if self.files.len() > SLOTS.len() {
            errors.push(("images", "announcements.images.too_many"));
        }
        let mut seen: Vec<u8> = Vec::new();
        for f in std::mem::take(&mut self.files) {
            // An unknown slot, or one posted twice: more than the form has.
            let Some(n) = slot_of(&f.field).filter(|n| !seen.contains(n)) else {
                errors.push(("images", "announcements.images.too_many"));
                continue;
            };
            seen.push(n);
            let field = SLOTS[usize::from(n - 1)];
            if f.bytes.len() > MAX_IMAGE_BYTES {
                errors.push((field, "announcements.images.too_big"));
                continue;
            }
            match sniff(&f.bytes, &f.file_name) {
                Some((_, size)) if !size_ok(size) => {
                    errors.push((field, "announcements.images.too_many_pixels"));
                }
                Some((kind, size)) => edits.put.push(NewFile {
                    kind: IMAGE,
                    n,
                    content_type: kind.content_type(),
                    ext: kind.ext(),
                    size: Some(size),
                    name: format!("image-{n}.{}", kind.ext()),
                    bytes: f.bytes,
                }),
                None => errors.push((field, "announcements.images.bad_type")),
            }
        }
        removed.retain(|n| !edits.put.iter().any(|p| p.n == *n));
        removed.sort_unstable();
        removed.dedup();
        edits.remove = removed.into_iter().map(|n| (IMAGE, n)).collect();
        if errors.is_empty() {
            Ok(edits)
        } else {
            Err(super::form::unique(errors))
        }
    }

    async fn read(mut m: Multipart) -> Result<Self, StatusCode> {
        let mut out = Posted::default();
        while let Some(mut field) = m.next_field().await.map_err(status)? {
            let name = field.name().unwrap_or_default().to_string();
            match field.file_name().map(str::to_string) {
                Some(file_name) => {
                    let mut bytes = Vec::new();
                    while let Some(chunk) = field.chunk().await.map_err(status)? {
                        // Keep one byte past the limit: enough to refuse.
                        let max = super::files_pdf::read_limit(&name).unwrap_or(MAX_IMAGE_BYTES);
                        let room = (max + 1).saturating_sub(bytes.len());
                        bytes.extend_from_slice(&chunk[..chunk.len().min(room)]);
                    }
                    // No file chosen: browsers still send an empty part.
                    if !bytes.is_empty() {
                        out.files.push(FilePart {
                            field: name,
                            file_name,
                            bytes,
                        });
                    }
                }
                None => {
                    let text = field.text().await.map_err(status)?;
                    out.pairs.push((name, text));
                }
            }
        }
        Ok(out)
    }
}

/// `image3` (or `image3` before `_remove`) → 3.
fn slot_of(field: &str) -> Option<u8> {
    let i = SLOTS.iter().position(|s| *s == field)?;
    u8::try_from(i + 1).ok()
}

fn status(e: axum::extract::multipart::MultipartError) -> StatusCode {
    e.status()
}

impl<S: Send + Sync> FromRequest<S> for Posted {
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let multipart = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("multipart/form-data"));
        if multipart {
            let m = Multipart::from_request(req, state)
                .await
                .map_err(IntoResponse::into_response)?;
            return Posted::read(m).await.map_err(IntoResponse::into_response);
        }
        let Form(pairs) = Form::<Vec<(String, String)>>::from_request(req, state)
            .await
            .map_err(IntoResponse::into_response)?;
        Ok(Posted {
            pairs,
            files: Vec::new(),
        })
    }
}

/// The images fieldset: the hint, one file input per slot, and for a
/// saved image its thumbnail and a "remove" box.
pub(super) fn field(
    v: &View<'_>,
    images: &[Stored],
    bodies: &[&str],
    sent: bool,
    reselect: bool,
    err: &dyn Fn(&str) -> Markup,
) -> Markup {
    html! {
        fieldset class="announcement-images" {
            legend { (v.t("announcements.images.legend")) }
            p class="hint" { (v.t("announcements.images.hint")) }
            (hero_warning(v, images, bodies))
            p class="muted" { (v.t("announcements.images.place")) }
            @if v.s.config.public_url.is_none() {
                p class="muted" { (v.t("announcements.images.no_public_url")) }
            }
            (err("images"))
            @if reselect {
                p class="field-error" role="alert" { (v.t("announcements.images.reselect")) }
            }
            @for (i, name) in SLOTS.iter().enumerate() {
                @let n = (i + 1) as u8;
                div class="field image-slot" data-image-slot=(n) data-filled[slot(images, n).is_some()] {
                    label for=(name) { (v.t("announcements.images.slot")) " " (n) }
                    (err(name))
                    @if let Some(f) = slot(images, n) {
                        (f.img(true).markup(&format!("image:{n}")))
                        label class="check" {
                            input type="checkbox" name=(format!("{name}_remove")) value="1";
                            " " (v.t("announcements.images.remove"))
                        }
                        // Removed from a sent one, it stays on its URL for
                        // the Cards already delivered (Staff may be
                        // removing it for personal data: say so).
                        @if sent {
                            p class="field-warning" { (v.t("announcements.images.remove_sent")) }
                        }
                    }
                    input id=(name) type="file" name=(name) accept="image/jpeg,image/png";
                }
            }
        }
    }
}

/// A warning, never a refusal, when the image a chat Card shows (for any
/// of the `bodies`: each Locale places its own first image) is over
/// [`HERO_SIDE`] pixels a side: LINE's Flex image limit (#109).
pub(super) fn hero_warning(v: &View<'_>, images: &[Stored], bodies: &[&str]) -> Markup {
    let large = bodies.iter().any(|body| {
        super::files::hero(images, body)
            .and_then(|f| f.size)
            .is_some_and(|(w, h)| w > HERO_SIDE || h > HERO_SIDE)
    });
    html! {
        @if large { p class="field-warning" role="status" { (v.t("announcements.images.hero_large")) } }
    }
}
