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
