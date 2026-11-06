//! Default texts of the admin pages, in Thai and English: shared words,
//! Security Events, the audit log and Texts here; Staff and Roles in
//! `texts_staff.rs`.

use crate::manifest::CatalogEntry as E;

pub(super) use super::texts_staff::STAFF;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}
pub(super) use th_en;

pub(super) const DEFAULTS: &[E] = th_en![
    // Shared
    ("kernel.admin.all", "ทั้งหมด", "All"),
    ("kernel.admin.at", "เวลา", "Time"),
    ("kernel.admin.from", "ตั้งแต่วันที่", "From"),
    ("kernel.admin.to", "ถึงวันที่", "To"),
    ("kernel.admin.filter", "กรอง", "Filter"),
    ("kernel.admin.clear", "ล้างตัวกรอง", "Clear"),
    ("kernel.admin.total", "ทั้งหมด", "Total"),
    ("kernel.admin.none", "ไม่พบรายการ", "Nothing found"),
    ("kernel.admin.prev", "ก่อนหน้า", "Previous"),
    ("kernel.admin.next", "ถัดไป", "Next"),
    ("kernel.admin.page", "หน้า", "Page"),
    ("kernel.admin.back", "กลับ", "Back"),
    ("kernel.admin.save", "บันทึก", "Save"),
    ("kernel.admin.saved", "บันทึกแล้ว", "Saved"),
    (
        "kernel.admin.not_found",
        "ไม่พบรายการนี้",
        "There is no such item"
    ),
    (
        "kernel.admin.conflict",
        "ข้อมูลถูกแก้ไขไปแล้วระหว่างนี้ กรุณาตรวจสอบแล้วลองอีกครั้ง",
        "Someone changed this in the meantime. Please check and try again."
    ),
    // Security Events
    (
        "kernel.admin.security.title",
        "เหตุการณ์ความปลอดภัย",
        "Security Events"
    ),
    ("kernel.admin.security.kind", "ประเภท", "Kind"),
    (
        "kernel.admin.security.student",
        "นักศึกษา (รหัสหรือชื่อ)",
        "Student (ID or name)"
    ),
    ("kernel.admin.security.account", "บัญชีแชท", "Chat account"),
    ("kernel.admin.security.staff", "เจ้าหน้าที่", "Staff"),
    ("kernel.admin.security.details", "รายละเอียด", "Details"),
    (
        "kernel.admin.kind.otp_lockout",
        "ใส่รหัสผิดเกินกำหนด",
        "Too many wrong codes"
    ),
    (
        "kernel.admin.kind.link_duplicate_attempt",
        "พยายามเชื่อมรหัสที่เชื่อมแล้ว",
        "Claim of a linked student ID"
    ),
    (
        "kernel.admin.kind.inbound_failed",
        "รับข้อความไม่สำเร็จ",
        "Inbound failed"
    ),
    (
        "kernel.admin.kind.otp_send_failed",
        "ส่งรหัสทางอีเมลไม่สำเร็จ",
        "Code email failed"
    ),
    (
        "kernel.admin.kind.staff_lockout",
        "เจ้าหน้าที่ถูกล็อก",
        "Staff account locked"
    ),
    // Audit log
    ("kernel.admin.audit.title", "บันทึกการใช้งาน", "Audit log"),
    ("kernel.admin.audit.actor", "ผู้กระทำ", "Actor"),
    ("kernel.admin.audit.action", "การกระทำ", "Action"),
    ("kernel.admin.audit.target", "เป้าหมาย", "Target"),
    ("kernel.admin.audit.ip", "IP", "IP"),
    ("kernel.admin.audit.request", "รหัสคำขอ", "Request id"),
    ("kernel.admin.audit.user_agent", "เบราว์เซอร์", "User agent"),
    (
        "kernel.admin.audit.filter_help",
        "ผู้กระทำ: อีเมลเจ้าหน้าที่ หรือ staff, student, system, job (ใส่ :รหัส ต่อท้ายได้) · การกระทำ: ขึ้นต้นด้วย เช่น student. · เป้าหมาย: ชนิด หรือ ชนิด:รหัส",
        "Actor: a Staff email, or staff, student, system, job (optionally :id) · Action: starts with, e.g. student. · Target: kind or kind:id"
    ),
    ("kernel.admin.audit.back", "กลับไปที่บันทึก", "Back to the log"),
    (
        "kernel.admin.audit.changes",
        "ก่อนและหลัง",
        "Before and after"
    ),
    ("kernel.admin.audit.key", "ช่อง", "Field"),
    ("kernel.admin.audit.before", "ก่อน", "Before"),
    ("kernel.admin.audit.after", "หลัง", "After"),
    ("kernel.admin.audit.changed", "เปลี่ยน", "changed"),
    (
        "kernel.admin.audit.no_values",
        "รายการนี้ไม่มีค่าก่อนและหลัง",
        "This row has no before or after values"
    ),
    // Texts
    ("kernel.admin.texts.title", "ข้อความ", "Texts"),
    (
        "kernel.admin.texts.search",
        "ค้นหาจากคีย์หรือข้อความ",
        "Search by key or text"
    ),
    ("kernel.admin.texts.key", "คีย์", "Key"),
    ("kernel.admin.texts.back", "กลับไปที่ข้อความ", "Back to texts"),
    ("kernel.admin.texts.default", "ค่าตั้งต้น", "Default"),
    ("kernel.admin.texts.value", "ข้อความที่ใช้", "Text in use"),
    (
        "kernel.admin.texts.placeholders",
        "ต้องมีตัวแทนค่า:",
        "Required placeholders:"
    ),
    ("kernel.admin.texts.source_default", "ค่าตั้งต้น", "default"),
    (
        "kernel.admin.texts.source_toml",
        "deployment.toml",
        "deployment.toml"
    ),
    (
        "kernel.admin.texts.source_setting",
        "แก้ในหลังบ้าน",
        "edited here"
    ),
    (
        "kernel.admin.texts.reset",
        "กลับไปใช้ค่าตั้งต้น",
        "Reset to default"
    ),
    (
        "kernel.admin.texts.reset_done",
        "กลับไปใช้ค่าตั้งต้นแล้ว",
        "Back to the default"
    ),
    (
        "kernel.admin.texts.err_empty",
        "ข้อความต้องไม่ว่าง",
        "The text cannot be empty"
    ),
    (
        "kernel.admin.texts.err_long",
        "ข้อความยาวเกิน 5000 ตัวอักษร",
        "The text is longer than 5000 characters"
    ),
    (
        "kernel.admin.texts.err_missing",
        "ข้อความขาดตัวแทนค่า:",
        "The text drops placeholders:"
    ),
    (
        "kernel.admin.texts.err_unknown",
        "ข้อความมีตัวแทนค่าที่ไม่รู้จัก:",
        "The text adds unknown placeholders:"
    ),
    (
        "kernel.admin.texts.consent_title",
        "ฉบับของประกาศความเป็นส่วนตัว",
        "Privacy notice version"
    ),
    (
        "kernel.admin.texts.consent_current",
        "ฉบับปัจจุบัน:",
        "Current version:"
    ),
    (
        "kernel.admin.texts.consent_help",
        "เมื่อเปลี่ยนฉบับ นักศึกษาที่เชื่อมบัญชีแล้วจะได้รับประกาศให้ยอมรับใหม่ในข้อความถัดไป",
        "After a change, every linked Student is asked to accept the notice again with their next message."
    ),
    ("kernel.admin.texts.consent_new", "ฉบับใหม่", "New version"),
    (
        "kernel.admin.texts.consent_save",
        "เปลี่ยนฉบับ",
        "Change the version"
    ),
    (
        "kernel.admin.texts.consent_saved",
        "เปลี่ยนฉบับแล้ว นักศึกษาจะถูกขอความยินยอมใหม่",
        "Version changed: Students will be asked again"
    ),
    (
        "kernel.admin.texts.consent_invalid",
        "ฉบับต้องเป็นตัวอักษรอังกฤษ ตัวเลข . _ หรือ - ไม่เกิน 32 ตัว",
        "A version is up to 32 letters, digits, . _ or -"
    ),
    (
        "kernel.admin.texts.consent_same",
        "นี่คือฉบับปัจจุบันอยู่แล้ว",
        "That is already the current version"
    ),
];
