//! Default texts of the Students pages, in Thai and English.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const DEFAULTS: &[E] = th_en![
    ("kernel.students.title", "นักศึกษา", "Students"),
    (
        "kernel.students.search",
        "ค้นหาชื่อหรือรหัสนักศึกษา",
        "Search by name or student ID"
    ),
    ("kernel.students.total", "ทั้งหมด", "Total"),
    (
        "kernel.students.missing_filter",
        "เฉพาะนักศึกษาที่ข้อมูลไม่ครบ",
        "Only Students with missing data"
    ),
    ("kernel.students.missing_badge", "ข้อมูลไม่ครบ", "Missing data"),
    (
        "kernel.students.missing_no_email",
        "ไม่มีอีเมล (เริ่มเชื่อมบัญชีแชทไม่ได้)",
        "No email (cannot start linking a chat account)"
    ),
    (
        "kernel.students.missing_bad_email",
        "อีเมลไม่ถูกต้อง",
        "Invalid email"
    ),
    ("kernel.students.prev", "ก่อนหน้า", "Previous"),
    ("kernel.students.next", "ถัดไป", "Next"),
    ("kernel.students.page", "หน้า", "Page"),
    // Deprecated (#127): unused, kept so `[texts]` overrides still build.
    ("kernel.students.none", "ไม่พบนักศึกษา", "No Students found"),
    (
        "kernel.students.not_found",
        "ไม่พบนักศึกษาคนนี้",
        "There is no such Student"
    ),
    ("kernel.students.back", "กลับไปที่รายชื่อ", "Back to the list"),
    // Fields
    ("kernel.students.code", "รหัสนักศึกษา", "Student ID"),
    ("kernel.students.name", "ชื่อ-สกุล", "Name"),
    ("kernel.students.email", "อีเมล", "Email"),
    ("kernel.students.locale", "ภาษา", "Language"),
    ("kernel.students.status", "สถานะ", "Status"),
    ("kernel.students.links", "บัญชีแชท", "Chat accounts"),
    ("kernel.students.status_active", "ศึกษาอยู่", "Active"),
    ("kernel.students.status_left", "พ้นสภาพ", "Left"),
    ("kernel.students.no_value", "ไม่มีข้อมูล", "Not set"),
    // Link statuses and methods
    ("kernel.students.link_active", "เชื่อมอยู่", "Linked"),
    (
        "kernel.students.link_unreachable",
        "ติดต่อไม่ได้",
        "Unreachable"
    ),
    ("kernel.students.link_none", "ยังไม่เชื่อม", "Not linked"),
    ("kernel.students.link_revoked", "ยกเลิกแล้ว", "Revoked"),
    (
        "kernel.students.method_email_otp",
        "ยืนยันทางอีเมล",
        "Email code"
    ),
    (
        "kernel.students.method_manual",
        "เจ้าหน้าที่ผูกให้",
        "Manual Link"
    ),
    ("kernel.students.linked_at", "เชื่อมเมื่อ", "Linked at"),
    ("kernel.students.revoked_at", "ยกเลิกเมื่อ", "Revoked at"),
    ("kernel.students.verified_by", "ผูกโดย", "Linked by"),
    ("kernel.students.account", "รหัสบัญชี", "Account id"),
    ("kernel.students.history", "ประวัติการเชื่อม", "Link history"),
    // Sections
    ("kernel.students.profile", "ข้อมูลนักศึกษา", "Student details"),
    ("kernel.students.tags", "ป้าย", "Tags"),
    (
        "kernel.students.tags_roster",
        "จากทะเบียน",
        "From the roster"
    ),
    (
        "kernel.students.tags_manual",
        "เพิ่มโดยเจ้าหน้าที่",
        "Added by Staff"
    ),
    ("kernel.students.no_tags", "ไม่มีป้าย", "No Tags"),
    ("kernel.students.tag_name", "ชื่อป้าย", "Tag name"),
    ("kernel.students.tag_add", "เพิ่มป้าย", "Add Tag"),
    ("kernel.students.tag_remove", "เอาออก", "Remove"),
    ("kernel.students.consent", "ความยินยอม", "Consent"),
    ("kernel.students.consent_version", "ฉบับที่", "Version"),
    (
        "kernel.students.consent_none",
        "ยังไม่เคยให้ความยินยอม",
        "No Consent recorded"
    ),
    (
        "kernel.students.security",
        "เหตุการณ์ความปลอดภัยล่าสุด",
        "Recent Security Events"
    ),
    ("kernel.students.security_none", "ไม่มีเหตุการณ์", "No events"),
    // Edit
    ("kernel.students.edit", "แก้ไขข้อมูล", "Edit details"),
    ("kernel.students.save", "บันทึก", "Save"),
    ("kernel.students.cancel", "ยกเลิก", "Cancel"),
    ("kernel.students.override", "แก้ด้วยมือ", "Edited by hand"),
    (
        "kernel.students.use_roster",
        "ใช้ค่าจากทะเบียน",
        "Use the roster value"
    ),
    (
        "kernel.students.override_help",
        "ช่องที่แก้ด้วยมือจะไม่ถูกทับเมื่อนำเข้าทะเบียน จนกว่าจะกด \"ใช้ค่าจากทะเบียน\"",
        "Fields edited by hand are kept by roster imports until you press \"Use the roster value\"."
    ),
    (
        "kernel.students.code_readonly",
        "แก้รหัสนักศึกษาไม่ได้",
        "The student ID cannot be changed"
    ),
    (
        "kernel.students.fix_errors",
        "กรุณาแก้ช่องที่มีข้อผิดพลาด",
        "Please fix the fields marked below"
    ),
    (
        "kernel.students.err_required",
        "ต้องกรอกช่องนี้",
        "This field is required"
    ),
    (
        "kernel.students.err_integer",
        "ต้องเป็นจำนวนเต็ม",
        "Must be a whole number"
    ),
    (
        "kernel.students.err_number",
        "ต้องเป็นตัวเลข",
        "Must be a number"
    ),
    (
        "kernel.students.err_date",
        "ต้องเป็นวันที่ (ปปปป-ดด-วว)",
        "Must be a date (YYYY-MM-DD)"
    ),
    (
        "kernel.students.err_range",
        "ค่าอยู่นอกช่วงที่กำหนด",
        "Outside the allowed range"
    ),
    (
        "kernel.students.err_enum",
        "ต้องเลือกจากรายการ",
        "Must be one of the listed values"
    ),
    (
        "kernel.students.err_email",
        "รูปแบบอีเมลไม่ถูกต้อง",
        "This is not a valid email"
    ),
    (
        "kernel.students.err_locale",
        "รหัสภาษาไม่ถูกต้อง",
        "This is not a valid language code"
    ),
    ("kernel.students.err_too_long", "ยาวเกินไป", "Too long"),
    // Status
    ("kernel.students.set_left", "เปลี่ยนเป็นพ้นสภาพ", "Mark as left"),
    (
        "kernel.students.set_active",
        "เปลี่ยนเป็นศึกษาอยู่",
        "Mark as active"
    ),
    (
        "kernel.students.confirm_left",
        "เปลี่ยนสถานะเป็นพ้นสภาพ? บัญชีแชททุกช่องทางของนักศึกษาคนนี้จะถูกยกเลิกการเชื่อมและได้รับแจ้ง",
        "Mark this Student as left? Every chat account they linked is unlinked and notified."
    ),
    (
        "kernel.students.confirm_active",
        "เปลี่ยนสถานะเป็นศึกษาอยู่? นักศึกษาต้องเชื่อมบัญชีใหม่เอง",
        "Mark this Student as active? They must link their accounts again themselves."
    ),
    ("kernel.students.confirm", "ยืนยัน", "Confirm"),
    // Manual Link and revoke
    (
        "kernel.students.manual_link",
        "ผูกบัญชีโดยเจ้าหน้าที่",
        "Manual Link"
    ),
    (
        "kernel.students.manual_link_help",
        "ตรวจสอบตัวตนนักศึกษาก่อน บัญชีต้องเคยทักบอทในช่องทางนั้นมาแล้ว",
        "Check the Student's identity first. The account must have written to the bot on that channel."
    ),
    ("kernel.students.channel", "ช่องทาง", "Channel"),
    (
        "kernel.students.external_id",
        "รหัสบัญชีในช่องทาง",
        "Account id on the channel"
    ),
    ("kernel.students.link_button", "ผูกบัญชี", "Link"),
    // The Manual Link picker (#143)
    (
        "kernel.students.pick_title",
        "บัญชีแชทที่ยังไม่ได้เชื่อม (ล่าสุดอยู่บน)",
        "Chat accounts not linked yet (newest first)"
    ),
    (
        "kernel.students.pick_hint",
        "ขอให้นักศึกษาส่งข้อความอะไรก็ได้มาที่บอทตอนนี้ แล้วเลือกบัญชีบนสุด",
        "Ask the Student to send any message to the bot now, then pick the top account."
    ),
    (
        "kernel.students.pick_none",
        "ยังไม่มีบัญชีที่ทักบอทแล้วยังไม่ได้เชื่อม ขอให้นักศึกษาทักบอทก่อน แล้วโหลดหน้านี้ใหม่",
        "No unlinked account has written to the bot yet. Ask the Student to write to it, then reload this page."
    ),
    (
        "kernel.students.pick_capped",
        "แสดงเฉพาะ 20 บัญชีล่าสุด ถ้าไม่พบ ให้พิมพ์รหัสบัญชีเองด้านล่าง",
        "Only the 20 newest accounts are shown. If it is not here, type the account id below."
    ),
    ("kernel.students.pick_last", "ทักล่าสุด", "Last active"),
    ("kernel.students.pick_first", "ทักครั้งแรก", "First wrote"),
    (
        "kernel.students.pick_stage_new",
        "ยังไม่เริ่มเชื่อมบัญชี",
        "Has not started linking"
    ),
    (
        "kernel.students.pick_stage_consent",
        "ยังไม่ยอมรับประกาศความเป็นส่วนตัว",
        "Has not accepted the privacy notice"
    ),
    (
        "kernel.students.pick_stage_student_id",
        "รอรหัสนักศึกษา",
        "Waiting for the student ID"
    ),
    (
        "kernel.students.pick_stage_otp",
        "รอรหัสยืนยันจากอีเมล",
        "Waiting for the emailed code"
    ),
    (
        "kernel.students.pick_blocked",
        "บล็อกบอทอยู่",
        "Has blocked the bot"
    ),
    (
        "kernel.students.pick_typed",
        "พิมพ์รหัสบัญชีเอง",
        "Type the account id instead"
    ),
    (
        "kernel.students.pick_both",
        "ผูกไม่ได้: เลือกบัญชีจากรายการ หรือพิมพ์รหัสบัญชีเอง อย่างใดอย่างหนึ่ง",
        "Cannot link: pick an account from the list or type an id, not both"
    ),
    ("kernel.students.revoke", "ยกเลิกการเชื่อม", "Revoke"),
    (
        "kernel.students.confirm_revoke",
        "ยกเลิกการเชื่อมบัญชีนี้? นักศึกษาจะได้รับแจ้งและเมนูจะถูกล็อก",
        "Revoke this Link? The Student is notified and the Menu is locked."
    ),
    // Flashes
    ("kernel.students.saved", "บันทึกแล้ว", "Saved"),
    (
        "kernel.students.no_change",
        "ไม่มีอะไรเปลี่ยน",
        "Nothing changed"
    ),
    (
        "kernel.students.conflict",
        "ข้อมูลถูกเปลี่ยนโดยผู้อื่นระหว่างนี้ กรุณาตรวจสอบแล้วลองใหม่",
        "Someone else changed this Student meanwhile. Please check and try again."
    ),
    (
        "kernel.students.override_cleared",
        "จะใช้ค่าจากทะเบียนในการนำเข้าครั้งถัดไป",
        "The next roster import will set this field"
    ),
    ("kernel.students.tag_added", "เพิ่มป้ายแล้ว", "Tag added"),
    ("kernel.students.tag_removed", "เอาป้ายออกแล้ว", "Tag removed"),
    (
        "kernel.students.tag_invalid",
        "ชื่อป้ายต้องยาว 1-64 ตัวอักษร",
        "A Tag name is 1 to 64 characters"
    ),
    (
        "kernel.students.tag_exists",
        "นักศึกษามีป้ายนี้อยู่แล้ว",
        "The Student already has this Tag"
    ),
    (
        "kernel.students.status_saved",
        "เปลี่ยนสถานะแล้ว",
        "Status changed"
    ),
    (
        "kernel.students.status_saved_unnotified",
        "เปลี่ยนสถานะแล้ว แต่แจ้งนักศึกษาได้ไม่ครบทุกบัญชี",
        "Status changed, but not every chat account could be notified"
    ),
    (
        "kernel.students.linked",
        "ผูกบัญชีแล้ว และแจ้งนักศึกษาแล้ว",
        "Linked, and the Student was notified"
    ),
    (
        "kernel.students.linked_unnotified",
        "ผูกบัญชีแล้ว แต่ส่งข้อความแจ้งนักศึกษาไม่สำเร็จ",
        "Linked, but the notice could not be delivered"
    ),
    (
        "kernel.students.link_invalid",
        "ผูกไม่ได้: ไม่พบบัญชีนี้ในช่องทางที่เลือก (ต้องเคยทักบอทมาก่อน) หรือนักศึกษาพ้นสภาพแล้ว",
        "Cannot link: no such account on that channel (it must have written to the bot), or the Student has left"
    ),
    (
        "kernel.students.link_dev_only",
        "ผูกไม่ได้: ช่องทางนี้มีไว้ทดสอบระบบเท่านั้น เลือกช่องทางอื่น",
        "Cannot link: this channel is for testing only; choose another channel"
    ),
    (
        "kernel.students.link_conflict",
        "ผูกไม่ได้: นักศึกษามีบัญชีที่เชื่อมอยู่ในช่องทางนี้แล้ว หรือบัญชีนี้เชื่อมกับคนอื่นอยู่",
        "Cannot link: the Student already has a live Link on this channel, or the account is linked to someone else"
    ),
    (
        "kernel.students.revoked",
        "ยกเลิกการเชื่อมแล้ว และแจ้งนักศึกษาแล้ว",
        "Revoked, and the Student was notified"
    ),
    (
        "kernel.students.revoked_unnotified",
        "ยกเลิกการเชื่อมแล้ว แต่ส่งข้อความแจ้งนักศึกษาไม่สำเร็จ",
        "Revoked, but the notice could not be delivered"
    ),
    (
        "kernel.students.already_revoked",
        "การเชื่อมนี้ถูกยกเลิกไปแล้ว",
        "This Link is already revoked"
    ),
];
