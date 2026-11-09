//! Default texts of the audit log in Staff's words (#125): each Kernel and
//! announcements action as a short Thai sentence, and each kind of actor
//! and target. See `audit_words.rs`.

use super::texts::th_en;
use crate::manifest::CatalogEntry as E;

pub(super) const AUDIT: &[E] = th_en![
    // Kinds of actor and target.
    ("kernel.admin.audit.kind.staff", "เจ้าหน้าที่", "Staff"),
    ("kernel.admin.audit.kind.student", "นักศึกษา", "Student"),
    ("kernel.admin.audit.kind.system", "ระบบ", "System"),
    (
        "kernel.admin.audit.kind.job",
        "งานเบื้องหลังของระบบ",
        "Background job"
    ),
    (
        "kernel.admin.audit.kind.announcements",
        "ประกาศ",
        "Announcement"
    ),
    (
        "kernel.admin.audit.kind.roster_import",
        "การนำเข้ารายชื่อ",
        "Roster import"
    ),
    ("kernel.admin.audit.kind.role", "บทบาท", "Role"),
    (
        "kernel.admin.audit.kind.setting",
        "ข้อความหรือค่าตั้งค่า",
        "Text or setting"
    ),
    (
        "kernel.admin.audit.kind.audit_log",
        "บันทึกการใช้งาน",
        "Audit log"
    ),
    (
        "kernel.admin.audit.kind.backup_run",
        "ชุดสำรองข้อมูล",
        "Backup"
    ),
    ("kernel.admin.audit.kind.channel", "ช่องทาง", "Channel"),
    ("kernel.admin.audit.kind.link", "การผูกบัญชี", "Link"),
    (
        "kernel.admin.audit.kind.module_upload",
        "ไฟล์ที่อัปโหลด",
        "Uploaded file"
    ),
    (
        "kernel.admin.audit.kind.security_event",
        "เหตุการณ์ความปลอดภัย",
        "Security Event"
    ),
    // Actions.
    (
        "kernel.admin.audit.action.module_upload",
        "อัปโหลดไฟล์",
        "Uploaded a file"
    ),
    (
        "kernel.admin.audit.action.module_upload_confirm",
        "ยืนยันนำเข้าไฟล์ที่อัปโหลด",
        "Confirmed an uploaded file"
    ),
    (
        "kernel.admin.audit.action.module_upload_cancel",
        "ยกเลิกไฟล์ที่อัปโหลด",
        "Cancelled an uploaded file"
    ),
    (
        "kernel.admin.audit.action.announcement_create",
        "สร้างประกาศ",
        "Created an announcement"
    ),
    (
        "kernel.admin.audit.action.announcement_update",
        "แก้ไขประกาศ",
        "Edited an announcement"
    ),
    (
        "kernel.admin.audit.action.announcement_send",
        "ส่งประกาศ",
        "Sent an announcement"
    ),
    (
        "kernel.admin.audit.action.announcement_schedule",
        "ตั้งเวลาส่งประกาศ",
        "Scheduled an announcement"
    ),
    (
        "kernel.admin.audit.action.announcement_cancel",
        "ยกเลิกเวลาส่งประกาศ",
        "Cancelled a schedule"
    ),
    (
        "kernel.admin.audit.action.announcement_resend",
        "ส่งประกาศอีกครั้ง",
        "Sent an announcement again"
    ),
    (
        "kernel.admin.audit.action.announcement_retract",
        "ถอนประกาศ",
        "Withdrew an announcement"
    ),
    (
        "kernel.admin.audit.action.announcement_notice",
        "แจ้งนักศึกษาว่าประกาศถูกแก้ไข",
        "Sent an edit notice"
    ),
    (
        "kernel.admin.audit.action.audit_view",
        "ดูบันทึกการใช้งาน",
        "Viewed the audit log"
    ),
    (
        "kernel.admin.audit.action.audit_detail",
        "เปิดดูรายการในบันทึกการใช้งาน",
        "Opened an audit entry"
    ),
    (
        "kernel.admin.audit.action.security_view",
        "ดูเหตุการณ์ความปลอดภัย",
        "Viewed Security Events"
    ),
    (
        "kernel.admin.audit.action.backup_create",
        "สำรองข้อมูล",
        "Backed up"
    ),
    (
        "kernel.admin.audit.action.backup_request",
        "สั่งสำรองข้อมูล",
        "Asked for a backup"
    ),
    (
        "kernel.admin.audit.action.backup_download",
        "ดาวน์โหลดชุดสำรองข้อมูล",
        "Downloaded a backup"
    ),
    (
        "kernel.admin.audit.action.backup_prune",
        "ลบชุดสำรองข้อมูลเก่า",
        "Deleted old backups"
    ),
    (
        "kernel.admin.audit.action.backup_restore",
        "กู้คืนข้อมูล",
        "Restored a backup"
    ),
    (
        "kernel.admin.audit.action.backup_notify",
        "แจ้งว่าสำรองข้อมูลไม่สำเร็จ",
        "Reported a failed backup"
    ),
    (
        "kernel.admin.audit.action.data_export",
        "ดาวน์โหลดข้อมูลทั้งหมด",
        "Downloaded all data"
    ),
    (
        "kernel.admin.audit.action.branding_update",
        "แก้ไขแบรนด์",
        "Changed the branding"
    ),
    (
        "kernel.admin.audit.action.branding_reset",
        "กลับไปใช้แบรนด์ตั้งต้น",
        "Reset the branding"
    ),
    (
        "kernel.admin.audit.action.branding_menu_images",
        "เปลี่ยนรูปเมนู LINE",
        "Changed the menu images"
    ),
    (
        "kernel.admin.audit.action.branding_menu_images_reset",
        "กลับไปใช้รูปเมนูตั้งต้น",
        "Reset the menu images"
    ),
    (
        "kernel.admin.audit.action.consent_accept",
        "ยอมรับประกาศความเป็นส่วนตัว",
        "Accepted the privacy notice"
    ),
    (
        "kernel.admin.audit.action.link_start",
        "เริ่มยืนยันตัวตน",
        "Started Verification"
    ),
    (
        "kernel.admin.audit.action.link_create",
        "ผูกบัญชีแชท",
        "Linked a chat account"
    ),
    (
        "kernel.admin.audit.action.link_revoke",
        "ยกเลิกการผูกบัญชี",
        "Revoked a Link"
    ),
    (
        "kernel.admin.audit.action.link_duplicate_attempt",
        "พยายามผูกรหัสที่ผูกแล้ว",
        "Claimed a linked student ID"
    ),
    (
        "kernel.admin.audit.action.link_shared_email",
        "พยายามผูกรหัสที่อีเมลซ้ำกับคนอื่น",
        "Claimed a student ID with a shared email"
    ),
    (
        "kernel.admin.audit.action.role_create",
        "สร้างบทบาท",
        "Created a Role"
    ),
    (
        "kernel.admin.audit.action.role_update",
        "แก้ไขสิทธิ์ของบทบาท",
        "Changed a Role's Permissions"
    ),
    (
        "kernel.admin.audit.action.role_delete",
        "ลบบทบาท",
        "Deleted a Role"
    ),
    (
        "kernel.admin.audit.action.roster_import_upload",
        "อัปโหลดไฟล์รายชื่อ",
        "Uploaded a roster file"
    ),
    (
        "kernel.admin.audit.action.roster_import",
        "นำเข้ารายชื่อ",
        "Imported a roster"
    ),
    (
        "kernel.admin.audit.action.roster_import_cancel",
        "ยกเลิกการนำเข้ารายชื่อ",
        "Cancelled a roster import"
    ),
    (
        "kernel.admin.audit.action.roster_import_fail",
        "นำเข้ารายชื่อไม่สำเร็จ",
        "A roster import failed"
    ),
    (
        "kernel.admin.audit.action.roster_file_download",
        "ดาวน์โหลดไฟล์รายชื่อต้นฉบับ",
        "Downloaded a roster file"
    ),
    (
        "kernel.admin.audit.action.roster_changes_download",
        "ดาวน์โหลดรายการเปลี่ยนแปลงของการนำเข้ารายชื่อ",
        "Downloaded the changes of a roster import"
    ),
    (
        "kernel.admin.audit.action.roster_file_prune",
        "ลบไฟล์รายชื่อเก่า",
        "Deleted old roster files"
    ),
    (
        "kernel.admin.audit.action.setting_update",
        "แก้ไขข้อความหรือค่าตั้งค่า",
        "Changed a text or setting"
    ),
    (
        "kernel.admin.audit.action.setting_reset",
        "กลับไปใช้ค่าตั้งต้น",
        "Reset a text or setting"
    ),
    (
        "kernel.admin.audit.action.staff_create",
        "เพิ่มเจ้าหน้าที่",
        "Added a Staff member"
    ),
    (
        "kernel.admin.audit.action.staff_roles",
        "เปลี่ยนบทบาทของเจ้าหน้าที่",
        "Changed a Staff member's Roles"
    ),
    (
        "kernel.admin.audit.action.staff_disable",
        "ปิดบัญชีเจ้าหน้าที่",
        "Disabled a Staff account"
    ),
    (
        "kernel.admin.audit.action.staff_enable",
        "เปิดบัญชีเจ้าหน้าที่",
        "Enabled a Staff account"
    ),
    (
        "kernel.admin.audit.action.staff_unlock",
        "ปลดล็อกบัญชีเจ้าหน้าที่",
        "Unlocked a Staff account"
    ),
    (
        "kernel.admin.audit.action.staff_lockout",
        "บัญชีเจ้าหน้าที่ถูกล็อก",
        "A Staff account was locked"
    ),
    (
        "kernel.admin.audit.action.staff_login",
        "เข้าสู่ระบบ",
        "Signed in"
    ),
    (
        "kernel.admin.audit.action.staff_login_failed",
        "เข้าสู่ระบบไม่สำเร็จ",
        "Failed to sign in"
    ),
    (
        "kernel.admin.audit.action.staff_logout",
        "ออกจากระบบ",
        "Signed out"
    ),
    (
        "kernel.admin.audit.action.staff_password_change",
        "เปลี่ยนรหัสผ่าน",
        "Changed the password"
    ),
    (
        "kernel.admin.audit.action.staff_password_change_failed",
        "เปลี่ยนรหัสผ่านไม่สำเร็จ",
        "Failed to change the password"
    ),
    (
        "kernel.admin.audit.action.staff_password_reset",
        "ตั้งรหัสผ่านชั่วคราวให้เจ้าหน้าที่",
        "Set a temporary password"
    ),
    (
        "kernel.admin.audit.action.student_view",
        "ดูข้อมูลนักศึกษา",
        "Viewed a Student"
    ),
    (
        "kernel.admin.audit.action.student_update",
        "แก้ไขข้อมูลนักศึกษา",
        "Edited a Student"
    ),
    (
        "kernel.admin.audit.action.student_status",
        "เปลี่ยนสถานะนักศึกษา",
        "Changed a Student's status"
    ),
    (
        "kernel.admin.audit.action.student_tag_add",
        "เพิ่มป้ายให้นักศึกษา",
        "Added a Tag"
    ),
    (
        "kernel.admin.audit.action.student_tag_remove",
        "ลบป้ายของนักศึกษา",
        "Removed a Tag"
    ),
    (
        "kernel.admin.audit.action.student_override_clear",
        "ล้างค่าที่แก้ด้วยมือ",
        "Cleared a hand edit"
    ),
    (
        "kernel.admin.audit.action.usage_notify",
        "แจ้งเตือนโควตา",
        "Quota warning"
    ),
    (
        "kernel.admin.audit.action.usage_notify_done",
        "ส่งแจ้งเตือนโควตาแล้ว",
        "Quota warning sent"
    ),
];
