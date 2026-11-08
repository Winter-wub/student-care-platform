//! Default texts of `/admin/settings` (#75), in Thai and English.

use super::texts::th_en;
use crate::manifest::CatalogEntry as E;

pub(super) const TEXTS: &[E] = th_en![
    (
        "kernel.admin.module_settings.title",
        "ค่าตั้งค่าโมดูล",
        "Module settings"
    ),
    (
        "kernel.admin.module_settings.intro",
        "ค่าที่ใช้ทั้งระบบ แก้ไขได้เฉพาะผู้มีสิทธิ์ของค่านั้น ทุกการแก้ไขถูกบันทึกในบันทึกการใช้งาน",
        "Values for the whole Deployment. Only Staff holding a setting's own Permission can change it; every change is in the audit log."
    ),
    ("kernel.admin.module_settings.value", "ค่า", "Value"),
    (
        "kernel.admin.module_settings.saved_value",
        "แก้ไขแล้ว",
        "Saved"
    ),
    (
        "kernel.admin.module_settings.default_value",
        "ค่าเริ่มต้น",
        "Default"
    ),
    (
        "kernel.admin.module_settings.unset_value",
        "ยังไม่ได้ตั้งค่า",
        "Not set yet"
    ),
    (
        "kernel.admin.module_settings.unset_text",
        "ยังไม่ได้ตั้งค่า จนกว่าผู้มีสิทธิ์จะบันทึกค่านี้ นักศึกษาจะเห็นว่ายังไม่ได้ตั้งค่า",
        "Not set yet. Until Staff holding its Permission save a value, Students see it as not set."
    ),
    (
        "kernel.admin.module_settings.updated",
        "แก้ไขล่าสุด",
        "Last changed"
    ),
    (
        "kernel.admin.module_settings.read_only",
        "ดูได้อย่างเดียว ต้องมีสิทธิ์",
        "Read only; editing needs"
    ),
    (
        "kernel.admin.module_settings.kind_text",
        "ข้อความบรรทัดเดียว",
        "Text"
    ),
    (
        "kernel.admin.module_settings.kind_multiline",
        "ข้อความหลายบรรทัด",
        "Multi-line text"
    ),
    (
        "kernel.admin.module_settings.kind_number",
        "จำนวนเต็ม",
        "Whole number"
    ),
    ("kernel.admin.module_settings.kind_date", "วันที่", "Date"),
    (
        "kernel.admin.module_settings.kind_url",
        "ลิงก์ https",
        "https link"
    ),
    (
        "kernel.admin.module_settings.kind_account",
        "เลขที่บัญชี",
        "Account number"
    ),
    (
        "kernel.admin.module_settings.err_invisible",
        "มีอักขระที่มองไม่เห็นหรือสลับทิศทางข้อความ ลบออกแล้วพิมพ์ใหม่",
        "Contains invisible or text-direction characters; remove them and type it again"
    ),
    (
        "kernel.admin.module_settings.err_account",
        "ใช้ได้เฉพาะตัวเลข 0-9 ขีด (-) และช่องว่าง",
        "Use only digits 0-9, hyphens (-) and spaces"
    ),
    (
        "kernel.admin.module_settings.broken",
        "ค่าที่บันทึกไว้ไม่ผ่านการตรวจแล้ว นักศึกษาจะเห็นว่ายังไม่ได้ตั้งค่า กรุณาบันทึกใหม่",
        "The saved value no longer passes its check; Students see it as not set. Save it again."
    ),
    (
        "kernel.admin.module_settings.err_empty",
        "กรุณากรอกค่า",
        "Enter a value"
    ),
    (
        "kernel.admin.module_settings.err_long",
        "ยาวเกินไป",
        "Too long"
    ),
    (
        "kernel.admin.module_settings.err_text",
        "มีอักขระที่ใช้ไม่ได้ (เช่น ขึ้นบรรทัดใหม่ในช่องบรรทัดเดียว)",
        "Contains characters not allowed here (e.g. a line break in a one-line value)"
    ),
    (
        "kernel.admin.module_settings.err_number",
        "ต้องเป็นจำนวนเต็ม ไม่มีจุลภาคหรือทศนิยม",
        "Must be a whole number, without separators or decimals"
    ),
    (
        "kernel.admin.module_settings.err_date",
        "ต้องเป็นวันที่ในรูปแบบ ปปปป-ดด-วว",
        "Must be a date as YYYY-MM-DD"
    ),
    (
        "kernel.admin.module_settings.err_url",
        "ต้องเป็นลิงก์ที่ขึ้นต้นด้วย https://",
        "Must be a link starting with https://"
    ),
];
