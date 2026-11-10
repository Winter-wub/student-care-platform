//! The PDF part of the create / edit form (#113): slots `pdf1` to `pdf3`,
//! the Thai hint, and the checks that turn the posted PDFs into
//! [`FileEdits`]. A save takes one new PDF at most ([`PDFS_PER_SAVE`],
//! for a Worker's memory): the form shows one file input, for the first
//! empty slot, and the next file is added by saving again. The box
//! `pdfN_remove` empties a slot. Each file is at most [`MAX_PDF_BYTES`]
//! and sniffed ([`super::files_pdf::sniff`]).

use maud::{Markup, html};

use super::files::{
    FileEdits, MAX_PDF_BYTES, MAX_PDFS, NewFile, PDF, PDFS_PER_SAVE, Stored, pdf_slot,
};
use super::files_pdf::{CONTENT_TYPE, Refusal, safe_name, size_label, sniff};
use super::form_images::{FilePart, Posted};
use super::view::View;
use crate::manifest::CatalogEntry as E;

/// The slot fields, `pdf1` to `pdf3` (also the error fields).
const SLOTS: [&str; MAX_PDFS as usize] = ["pdf1", "pdf2", "pdf3"];

/// Whether `field` is one the PDFs part shows an error under: the whole
/// set (`pdfs`) or a slot (#140).
pub(super) fn erred_in(field: &str) -> bool {
    field == "pdfs" || SLOTS.contains(&field)
}

type Errors = Vec<(&'static str, &'static str)>;

/// PDF texts, Thai and English.
pub(crate) const TEXTS: &[E] = &[
    E::new("announcements.pdfs.legend", "th", "ไฟล์ PDF แนบ"),
    E::new("announcements.pdfs.legend", "en", "PDF attachments"),
    E::new(
        "announcements.pdfs.hint",
        "th",
        "แนบได้ไม่เกิน 3 ไฟล์ ไฟล์ละไม่เกิน 10 MB (PDF เท่านั้น) \
         ห้ามแนบไฟล์ที่มีข้อมูลส่วนตัว เช่น รายชื่อ เลขประจำตัว หรือผลการเรียนของนักศึกษา \
         เพราะใครมีลิงก์ก็เปิดไฟล์ได้ ไฟล์แรกจะเป็นปุ่มบนการ์ดในแชท",
    ),
    E::new(
        "announcements.pdfs.hint",
        "en",
        "Up to 3 files, 10 MB each (PDF only). Never attach personal data such as \
         Student names, ids or grades: anyone with the link can open the file. \
         The first file is a button on the chat Card.",
    ),
    E::new(
        "announcements.pdfs.no_public_url",
        "th",
        "ตอนนี้ไฟล์จะเปิดได้จากมินิแอปเท่านั้น การ์ดในแชทจะยังไม่มีปุ่มเปิดไฟล์ เพราะระบบยังไม่ได้ตั้งที่อยู่เว็บสาธารณะ (แจ้งผู้ดูแลระบบให้ตั้ง public_url)",
    ),
    E::new(
        "announcements.pdfs.no_public_url",
        "en",
        "public_url is not set: files open from the Mini App only, with no button on chat Cards.",
    ),
    E::new(
        "announcements.pdfs.once",
        "th",
        "แนบได้ครั้งละ 1 ไฟล์ บันทึกแล้วแนบไฟล์ถัดไปได้",
    ),
    E::new(
        "announcements.pdfs.once",
        "en",
        "One file per save: save, then attach the next one.",
    ),
    E::new(
        "announcements.pdfs.full",
        "th",
        "แนบครบ 3 ไฟล์แล้ว ลบไฟล์ใดไฟล์หนึ่งแล้วบันทึกก่อน จึงแนบไฟล์ใหม่ได้",
    ),
    E::new(
        "announcements.pdfs.full",
        "en",
        "3 files attached: remove one and save before attaching another.",
    ),
    E::new("announcements.pdfs.slot", "th", "ไฟล์ที่"),
    E::new("announcements.pdfs.slot", "en", "File"),
    E::new("announcements.pdfs.remove", "th", "ลบไฟล์นี้"),
    E::new("announcements.pdfs.remove", "en", "Remove this file"),
    E::new(
        "announcements.pdfs.remove_sent",
        "th",
        "ประกาศนี้ส่งแล้ว: ไฟล์ที่ลบจะหายจากหน้าและการ์ดใหม่ แต่ยังเปิดได้จากปุ่มเดิม\
         ในการ์ดที่ส่งไปแล้ว จนกว่าจะถอนประกาศหรือประกาศหมดอายุ \
         ถ้าไฟล์มีข้อมูลส่วนตัว ให้ถอนประกาศ",
    ),
    E::new(
        "announcements.pdfs.remove_sent",
        "en",
        "This announcement was sent: a removed file leaves the pages and new Cards, \
         but the button in the Cards already delivered still opens it until the \
         announcement is retracted or expires. If it holds personal data, retract the announcement.",
    ),
    E::new(
        "announcements.pdfs.bad_type",
        "th",
        "รับเฉพาะไฟล์ PDF ที่ชื่อลงท้ายด้วย .pdf (ตรวจจากเนื้อไฟล์ ไม่ใช่แค่ชื่อ)",
    ),
    E::new(
        "announcements.pdfs.bad_type",
        "en",
        "PDF only, named .pdf (checked by its content, not only its name).",
    ),
    E::new(
        "announcements.pdfs.active",
        "th",
        "ไฟล์ PDF นี้มีสคริปต์ ไฟล์ฝัง หรือคำสั่งเปิดโปรแกรม จึงรับไม่ได้ \
         ลองสั่งพิมพ์เป็น PDF ใหม่แล้วอัปโหลดอีกครั้ง",
    ),
    E::new(
        "announcements.pdfs.active",
        "en",
        "This PDF holds a script, an embedded file or a launch action: print it to a new PDF \
         and upload that.",
    ),
    E::new(
        "announcements.pdfs.unreadable",
        "th",
        "ตรวจไฟล์ PDF นี้ไม่ได้ (โครงสร้างไฟล์ไม่ครบหรือผิดรูปแบบ) \
         ลองสั่งพิมพ์เป็น PDF ใหม่แล้วอัปโหลดอีกครั้ง",
    ),
    E::new(
        "announcements.pdfs.unreadable",
        "en",
        "This PDF could not be checked (incomplete or malformed): print it to a new PDF \
         and upload that.",
    ),
    E::new(
        "announcements.pdfs.too_big",
        "th",
        "ไฟล์ใหญ่เกิน 10 MB ลดขนาดไฟล์แล้วอัปโหลดใหม่",
    ),
    E::new(
        "announcements.pdfs.too_big",
        "en",
        "Over 10 MB: make the file smaller and upload it again.",
    ),
    E::new("announcements.pdfs.too_many", "th", "แนบ PDF ได้ไม่เกิน 3 ไฟล์"),
    E::new("announcements.pdfs.too_many", "en", "At most 3 PDF files."),
    E::new(
        "announcements.pdfs.bad_slot",
        "th",
        "ช่องไฟล์ PDF มีเฉพาะที่ 1 ถึง 3",
    ),
    E::new("announcements.pdfs.bad_slot", "en", "PDF slots are 1 to 3."),
];

/// `pdf2` → 2.
fn slot_of(field: &str) -> Option<u8> {
    let i = SLOTS.iter().position(|s| *s == field)?;
    u8::try_from(i + 1).ok()
}

impl Posted {
    /// Every file edit of the form, the bytes moved out: the PDFs (#113)
    /// and the images (#109), or every problem of both, each once.
    pub(super) fn file_edits(&mut self) -> Result<FileEdits, Errors> {
        self.file_edits_up_to(PDFS_PER_SAVE)
    }

    /// [`Posted::file_edits`] taking up to `pdfs` new PDFs: one for a
    /// form post ([`PDFS_PER_SAVE`]), every slot from code (#164).
    pub(super) fn file_edits_up_to(&mut self, pdfs: usize) -> Result<FileEdits, Errors> {
        let (pdf_parts, rest) = std::mem::take(&mut self.files)
            .into_iter()
            .partition(|f| f.field.starts_with("pdf"));
        self.files = rest;
        let pdfs = pdf_edits(&self.pairs, pdf_parts, pdfs);
        match (self.image_edits(), pdfs) {
            (Ok(mut a), Ok(b)) => {
                a.put.extend(b.put);
                a.remove.extend(b.remove);
                Ok(a)
            }
            (Err(mut a), Err(b)) => {
                a.extend(b);
                Err(super::form::unique(a))
            }
            (Err(e), Ok(_)) | (Ok(_), Err(e)) => Err(e),
        }
    }
}

/// The PDF edits of the posted `pairs` (the remove boxes) and `parts`.
fn pdf_edits(
    pairs: &[(String, String)],
    parts: Vec<FilePart>,
    max: usize,
) -> Result<FileEdits, Errors> {
    let mut edits = FileEdits::default();
    let mut errors = Vec::new();
    let mut removed: Vec<u8> = pairs
        .iter()
        .filter_map(|(k, _)| k.strip_suffix("_remove").and_then(slot_of))
        .collect();
    if parts.len() > max {
        let key = if max == PDFS_PER_SAVE {
            "announcements.pdfs.once"
        } else {
            "announcements.pdfs.too_many"
        };
        errors.push(("pdfs", key));
        return Err(errors);
    }
    let mut seen: Vec<u8> = Vec::new();
    for f in parts {
        // An unknown slot (`pdf4`), or one posted twice.
        let Some(n) = slot_of(&f.field).filter(|n| !seen.contains(n)) else {
            errors.push(("pdfs", "announcements.pdfs.too_many"));
            continue;
        };
        seen.push(n);
        let field = SLOTS[usize::from(n - 1)];
        if f.bytes.len() > MAX_PDF_BYTES {
            errors.push((field, "announcements.pdfs.too_big"));
            continue;
        }
        match sniff(&f.bytes, &f.file_name) {
            Ok(()) => edits.put.push(NewFile {
                kind: PDF,
                n,
                content_type: CONTENT_TYPE,
                ext: "pdf",
                size: None,
                name: safe_name(&f.file_name, n),
                bytes: f.bytes,
            }),
            Err(Refusal::NotPdf) => errors.push((field, "announcements.pdfs.bad_type")),
            Err(Refusal::Active) => errors.push((field, "announcements.pdfs.active")),
            Err(Refusal::Unreadable) => errors.push((field, "announcements.pdfs.unreadable")),
        }
    }
    removed.retain(|n| !edits.put.iter().any(|p| p.n == *n));
    removed.sort_unstable();
    removed.dedup();
    edits.remove = removed.into_iter().map(|n| (PDF, n)).collect();
    if errors.is_empty() {
        Ok(edits)
    } else {
        Err(super::form::unique(errors))
    }
}

/// The PDF fieldset: the hint and the one-at-a-time line, each saved file
/// with its name, size and a "remove" box, and one file input for the
/// first empty slot (none once all are full). `files` holds every kind.
/// `reselect`: a PDF was chosen but the form came back without it (#140).
pub(super) fn field(
    v: &View<'_>,
    files: &[Stored],
    sent: bool,
    reselect: bool,
    err: &dyn Fn(&str) -> Markup,
) -> Markup {
    let next = (1..=MAX_PDFS).find(|n| pdf_slot(files, *n).is_none());
    html! {
        fieldset class="announcement-pdfs" aria-describedby=[reselect.then_some("err-reselect-pdfs")] {
            legend { (v.t("announcements.pdfs.legend")) }
            p class="hint" { (v.t("announcements.pdfs.hint")) }
            p class="muted" { (v.t("announcements.pdfs.once")) }
            @if v.s.config.public_url.is_none() {
                p class="muted" { (v.t("announcements.pdfs.no_public_url")) }
            }
            (err("pdfs"))
            @if reselect {
                // Not a live region (#129): the form's summary announces.
                p class="field-error" id="err-reselect-pdfs" { (v.t("announcements.images.reselect")) }
            }
            @for (i, name) in SLOTS.iter().enumerate() {
                @let n = (i + 1) as u8;
                @let saved = pdf_slot(files, n);
                @if saved.is_some() || next == Some(n) {
                div class="field pdf-slot" data-pdf-slot=(n) {
                    label for=(name) { (v.t("announcements.pdfs.slot")) " " (n) }
                    (err(name))
                    @if let Some(f) = saved {
                        p {
                            a href=(f.staff_path()) download { "📄 " (f.file_name) }
                            " (" (size_label(f.size_bytes)) ")"
                        }
                        label class="check" {
                            input type="checkbox" name=(format!("{name}_remove")) value="1";
                            " " (v.t("announcements.pdfs.remove"))
                        }
                        @if sent {
                            p class="field-warning" { (v.t("announcements.pdfs.remove_sent")) }
                        }
                    }
                    @if saved.is_none() {
                        input id=(name) type="file" name=(name) accept="application/pdf,.pdf";
                    }
                }
                } @else {
                    // A slot the form does not show (a posted `pdf2` on a
                    // new form): its error still shows.
                    (err(name))
                }
            }
            @if next.is_none() {
                p class="muted" { (v.t("announcements.pdfs.full")) }
            }
        }
    }
}
