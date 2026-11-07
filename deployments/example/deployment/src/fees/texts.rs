//! The Fees Module's texts (Thai default, English).

use harpseal::CatalogEntry;

/// Every text the pages use.
pub(super) const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("fees.title", "th", "ยอดค้างชำระ"),
    CatalogEntry::new("fees.title", "en", "Outstanding fees"),
    CatalogEntry::new("fees.file", "th", "ไฟล์ CSV หรือ XLSX (ไม่เกิน 4 MB)"),
    CatalogEntry::new("fees.file", "en", "CSV or XLSX file (up to 4 MB)"),
    CatalogEntry::new("fees.check", "th", "ตรวจไฟล์"),
    CatalogEntry::new("fees.check", "en", "Check the file"),
    CatalogEntry::new("fees.stored", "th", "ยอดที่บันทึกไว้"),
    CatalogEntry::new("fees.stored", "en", "Balances stored"),
    CatalogEntry::new("fees.preview", "th", "ตรวจก่อนนำเข้า"),
    CatalogEntry::new("fees.preview", "en", "Check before importing"),
    CatalogEntry::new("fees.confirm", "th", "ยืนยันนำเข้า"),
    CatalogEntry::new("fees.confirm", "en", "Confirm import"),
    CatalogEntry::new("fees.cancel", "th", "ยกเลิก"),
    CatalogEntry::new("fees.cancel", "en", "Cancel"),
    CatalogEntry::new("fees.problems", "th", "ปัญหาที่ต้องแก้ก่อนนำเข้า"),
    CatalogEntry::new("fees.problems", "en", "Fix these before importing"),
    CatalogEntry::new("fees.skipped", "th", "ข้ามไม่นำเข้า"),
    CatalogEntry::new("fees.skipped", "en", "Left out"),
    CatalogEntry::new("fees.gone", "th", "ไม่พบไฟล์นี้ หรือหมดเวลาแล้ว อัปโหลดใหม่"),
    CatalogEntry::new(
        "fees.gone",
        "en",
        "This file is gone or expired. Upload it again.",
    ),
    CatalogEntry::new("fees.changed", "th", "ไฟล์ถูกแก้ไขหลังอัปโหลด นำเข้าไม่ได้"),
    CatalogEntry::new(
        "fees.changed",
        "en",
        "The file changed after upload; it cannot be imported.",
    ),
    CatalogEntry::new("fees.unreadable", "th", "อ่านไฟล์ไม่ได้"),
    CatalogEntry::new("fees.unreadable", "en", "The file cannot be read."),
    CatalogEntry::new("fees.queued", "th", "ยืนยันแล้ว กำลังนำเข้า"),
    CatalogEntry::new("fees.queued", "en", "Confirmed; the import is queued."),
    CatalogEntry::new(
        "fees.failed",
        "th",
        "นำเข้าไม่สำเร็จ ติดต่อผู้ดูแลระบบ แล้วอัปโหลดใหม่",
    ),
    CatalogEntry::new(
        "fees.failed",
        "en",
        "The import failed. Ask an administrator, then upload again.",
    ),
];
