//! Default texts of the roster import warnings (#71), in Thai and English.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const DEFAULTS: &[E] = th_en![
    ("kernel.roster.warnings", "คำเตือน", "Warnings"),
    (
        "kernel.roster.warnings_help",
        "แถวเหล่านี้นำเข้าได้ แต่ควรตรวจก่อนยืนยัน รายงาน CSV มีครบทุกรายการ",
        "These rows import, but check them before you confirm. The CSV report lists every one."
    ),
    (
        "kernel.roster.warnings_shown",
        "แสดง {shown} จาก {total} รายการ ที่เหลืออยู่ในรายงาน CSV",
        "Showing {shown} of {total}; the rest are in the CSV report"
    ),
    (
        "kernel.roster.warnings_report",
        "ดาวน์โหลดรายงานคำเตือน (CSV)",
        "Download the warnings report (CSV)"
    ),
    (
        "kernel.roster.by_class",
        "จำนวนตามประเภท",
        "Count per class"
    ),
    ("kernel.roster.report_severity", "ระดับ", "Level"),
    ("kernel.roster.severity_blocking", "ต้องแก้", "Blocking"),
    ("kernel.roster.severity_warning", "เตือน", "Warning"),
    (
        "kernel.roster.reason_missing_email",
        "ไม่มีอีเมล: นักศึกษาคนนี้เริ่มเชื่อมบัญชีแชทไม่ได้จนกว่าเจ้าหน้าที่จะเพิ่มอีเมล",
        "No email: this Student cannot start linking a chat account until Staff add an email"
    ),
    (
        "kernel.roster.reason_invalid_email",
        "รูปแบบอีเมลไม่ถูกต้อง: นักศึกษาจะรับรหัสยืนยันทางอีเมลไม่ได้จนกว่าจะแก้",
        "The email is not valid: the Student cannot receive the verification code until it is fixed"
    ),
    (
        "kernel.roster.reason_duplicate_email",
        "อีเมลซ้ำกับนักศึกษาคนอื่น (ในไฟล์นี้หรือที่มีอยู่แล้วในระบบ)",
        "The email is also used by another Student (in this file or already in the system)"
    ),
    (
        "kernel.roster.reason_empty_attribute",
        "ช่องนี้ว่าง (ช่องที่เว้นว่างได้)",
        "Empty (an optional field)"
    ),
];
