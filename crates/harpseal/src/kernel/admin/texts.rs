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
    ("kernel.admin.prev", "ก่อนหน้า", "Previous"),
    ("kernel.admin.next", "ถัดไป", "Next"),
    ("kernel.admin.page", "หน้า", "Page"),
    // Deprecated (#127): unused, kept so `[texts]` overrides still build.
    ("kernel.admin.none", "ไม่พบรายการ", "Nothing found"),
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
    (
        "kernel.admin.bad_form",
        "แบบฟอร์มไม่ครบ กรุณาโหลดหน้าใหม่แล้วลองอีกครั้ง",
        "The form is incomplete. Please reload the page and try again."
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
        "kernel.admin.kind.link_shared_email",
        "พยายามเชื่อมรหัสที่อีเมลซ้ำกับคนอื่น",
        "Claim of a student ID whose email is shared"
    ),
    (
        "kernel.admin.kind.inbound_failed",
        "รับข้อความไม่สำเร็จ",
        "Inbound failed"
    ),
    (
        "kernel.admin.kind.inbound_lost",
        "ข้อความขาเข้าค้าง ไม่ได้ตอบ",
        "Inbound lost (never answered)"
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
        "ค้นหาข้อความ",
        "Search the texts"
    ),
    ("kernel.admin.texts.area", "หน้าที่แสดง", "Shown on"),
    ("kernel.admin.texts.all_areas", "ทุกหน้า", "Every page"),
    ("kernel.admin.texts.find", "ค้นหา", "Search"),
    (
        "kernel.admin.texts.intro",
        "ข้อความที่นักศึกษาและเจ้าหน้าที่เห็น จัดตามหน้าที่แสดง กดที่ข้อความเพื่อแก้ไข",
        "The texts Students and Staff see, grouped by where they show. Open one to edit it."
    ),
    ("kernel.admin.texts.shown_on", "แสดงที่:", "Shown on:"),
    (
        "kernel.admin.texts.area_chat",
        "แชทกับนักศึกษา",
        "Chat with Students"
    ),
    ("kernel.admin.texts.area_app", "มินิแอป", "Mini App"),
    ("kernel.admin.texts.area_menu", "เมนูในแชท", "Chat Menu"),
    ("kernel.admin.texts.area_backup", "สำรองข้อมูล", "Backups"),
    (
        "kernel.admin.texts.area_branding",
        "แบรนด์และรูปเมนู",
        "Branding and menu images"
    ),
    (
        "kernel.admin.texts.area_usage",
        "การใช้งานและโควตา",
        "Usage and Quotas"
    ),
    ("kernel.admin.texts.area_other", "ทั่วไป", "General"),
    ("kernel.admin.texts.area_module", "ส่วนเสริม", "Add-on"),
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
    ("kernel.admin.texts.source_toml", "ไฟล์ตั้งค่า", "config file"),
    // The technical details (#123).
    (
        "kernel.admin.texts.tech_toml",
        "ข้อความที่มีเครื่องหมาย \"ไฟล์ตั้งค่า\" มาจาก [texts] ใน deployment.toml ข้อความที่แก้ในหลังบ้านจะใช้แทนค่านั้น",
        "\"config file\" marks a text from [texts] in deployment.toml; an edit here is used instead."
    ),
    (
        "kernel.admin.texts.tech_key",
        "ชื่อคีย์ของข้อความ (ใช้อ้างอิงในโค้ดและไฟล์ตั้งค่า):",
        "The text's key (as code and the config file name it):"
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
        "ข้อความต้องไม่ว่าง กรุณาพิมพ์ข้อความแล้วบันทึกอีกครั้ง",
        "The text cannot be empty. Type a text and save again."
    ),
    (
        "kernel.admin.texts.err_long",
        "ข้อความยาวเกิน 5,000 ตัวอักษร กรุณาตัดให้สั้นลงแล้วบันทึกอีกครั้ง",
        "The text is longer than 5,000 characters. Shorten it and save again."
    ),
    (
        "kernel.admin.texts.err_missing",
        "ข้อความนี้ขาดตัวแทนค่าต่อไปนี้ ใส่กลับเข้าไปแล้วบันทึกอีกครั้ง:",
        "The text drops these placeholders. Put them back and save again:"
    ),
    (
        "kernel.admin.texts.err_unknown",
        "ข้อความมีตัวแทนค่าที่ระบบไม่รู้จัก ลบออกแล้วบันทึกอีกครั้ง:",
        "The text has placeholders the system does not know. Remove them and save again:"
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
        "เมื่อเปลี่ยนฉบับ นักศึกษาที่เชื่อมบัญชีแล้วจะได้รับประกาศให้ยอมรับใหม่ในข้อความถัดไป ยกเว้นฉบับที่นักศึกษาเคยยอมรับแล้ว ระบบจะไม่ถามซ้ำ",
        "After a change, every linked Student is asked to accept the notice again with their next message. A version a Student already accepted will not ask them again."
    ),
    ("kernel.admin.texts.consent_new", "ฉบับใหม่", "New version"),
    // Deprecated (#135): the card's button opens the review step now
    // (`kernel.admin.consent.review`). Kept so a `[texts]` override of it
    // still builds.
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
