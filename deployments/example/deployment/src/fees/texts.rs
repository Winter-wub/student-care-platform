//! The Fees Module's texts (Thai default, English).

use harpseal::CatalogEntry;

/// Every text the pages use.
pub(super) const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("fees.title", "th", "ยอดค้างชำระ"),
    CatalogEntry::new("fees.title", "en", "Outstanding fees"),
    // The audit log's words for `fees.import` (#125).
    CatalogEntry::new("fees.audit.import", "th", "นำเข้ายอดค้างชำระ"),
    CatalogEntry::new("fees.audit.import", "en", "Imported outstanding fees"),
    CatalogEntry::new("fees.file", "th", "ไฟล์ CSV หรือ XLSX (ไม่เกิน 4 MB)"),
    CatalogEntry::new("fees.file", "en", "CSV or XLSX file (up to 4 MB)"),
    // The upload's columns, for the help panel (#127).
    CatalogEntry::new("fees.col.code", "th", "รหัสนักศึกษา 8 หลัก"),
    CatalogEntry::new("fees.col.code", "en", "The 8-digit student ID"),
    CatalogEntry::new(
        "fees.col.name",
        "th",
        "ชื่อและนามสกุล ใช้ตรวจในหน้าก่อนนำเข้า ไม่ได้นำเข้า",
    ),
    CatalogEntry::new(
        "fees.col.name",
        "en",
        "First and last name, shown on the check page, not imported",
    ),
    CatalogEntry::new(
        "fees.col.amount",
        "th",
        "ยอดค้างชำระเป็นบาท ทศนิยมไม่เกิน 2 ตำแหน่ง",
    ),
    CatalogEntry::new(
        "fees.col.amount",
        "en",
        "The balance in baht, at most 2 decimals",
    ),
    CatalogEntry::new("fees.back_to_list", "th", "กลับไปหน้ายอดค้างชำระ"),
    CatalogEntry::new("fees.back_to_list", "en", "Back to outstanding fees"),
    // The result once imported, and the wait before it (#127).
    CatalogEntry::new("fees.imported_title", "th", "นำเข้ายอดค้างชำระแล้ว"),
    CatalogEntry::new("fees.imported_title", "en", "Outstanding fees imported"),
    CatalogEntry::new(
        "fees.imported",
        "th",
        "นำเข้ายอดค้างชำระ {n} รายการแล้ว เมื่อ {time}",
    ),
    CatalogEntry::new(
        "fees.imported",
        "en",
        "{n} balances were imported on {time}.",
    ),
    CatalogEntry::new(
        "fees.pending",
        "th",
        "แจ้งนักศึกษาได้เมื่อนำเข้าเสร็จ โหลดหน้านี้ใหม่อีกครั้งในอีกสักครู่",
    ),
    CatalogEntry::new(
        "fees.pending",
        "en",
        "You can tell Students once the import is done. Reload this page in a moment.",
    ),
    CatalogEntry::new(
        "fees.superseded",
        "th",
        "ยอดของนักศึกษาบางคนในไฟล์นี้ถูกแทนด้วยไฟล์ที่นำเข้าภายหลัง จึงต้องเลือกกลุ่มผู้รับเอง",
    ),
    CatalogEntry::new(
        "fees.superseded",
        "en",
        "A later file replaced some of these balances, so choose the Audience yourself.",
    ),
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
    // The flash after Cancel (#162).
    CatalogEntry::new("fees.cancelled", "th", "ยกเลิกแล้ว ลบไฟล์ที่อัปโหลดออกแล้ว"),
    CatalogEntry::new(
        "fees.cancelled",
        "en",
        "Cancelled. The uploaded file is deleted.",
    ),
    CatalogEntry::new(
        "fees.not_cancelled",
        "th",
        "ยกเลิกไม่ได้ ไฟล์นี้ยืนยันนำเข้าไปแล้ว หรือหมดเวลาแล้ว",
    ),
    CatalogEntry::new(
        "fees.not_cancelled",
        "en",
        "Not cancelled: this file was already confirmed for import, or it expired.",
    ),
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
    // The preview's column captions (#131): on a phone each cell names its column.
    CatalogEntry::new("fees.col_sheet", "th", "ชีต"),
    CatalogEntry::new("fees.col_sheet", "en", "Sheet"),
    CatalogEntry::new("fees.col_row", "th", "แถว"),
    CatalogEntry::new("fees.col_row", "en", "Row"),
];
