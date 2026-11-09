//! Default texts of the Staff and Roles pages, in Thai and English.

use super::texts::th_en;
use crate::manifest::CatalogEntry as E;

pub(crate) const STAFF: &[E] = th_en![
    ("kernel.admin.staff.title", "เจ้าหน้าที่", "Staff"),
    (
        "kernel.admin.staff.back",
        "กลับไปที่รายชื่อเจ้าหน้าที่",
        "Back to Staff"
    ),
    (
        "kernel.admin.staff.create",
        "เพิ่มเจ้าหน้าที่",
        "Add a Staff member"
    ),
    ("kernel.admin.staff.name", "ชื่อที่แสดง", "Display name"),
    ("kernel.admin.staff.email", "อีเมล", "Email"),
    (
        "kernel.admin.staff.locale",
        "ภาษาหลังบ้าน",
        "Back-office language"
    ),
    ("kernel.admin.staff.status", "สถานะ", "Status"),
    ("kernel.admin.staff.roles", "บทบาท", "Roles"),
    ("kernel.admin.staff.active", "ใช้งานได้", "Active"),
    ("kernel.admin.staff.disabled", "ปิดใช้งาน", "Disabled"),
    ("kernel.admin.staff.locked_until", "ล็อกถึง", "Locked until"),
    (
        "kernel.admin.staff.must_change",
        "ต้องเปลี่ยนรหัสผ่าน",
        "Must change password"
    ),
    (
        "kernel.admin.staff.temp_help",
        "ระบบจะสร้างรหัสผ่านชั่วคราวให้ แสดงครั้งเดียว และต้องเปลี่ยนเมื่อเข้าสู่ระบบครั้งแรก",
        "A temporary password is generated and shown once; it must be changed at the first sign-in."
    ),
    (
        "kernel.admin.staff.created",
        "เพิ่มเจ้าหน้าที่แล้ว",
        "Staff member added"
    ),
    (
        "kernel.admin.staff.reset_done",
        "ตั้งรหัสผ่านชั่วคราวแล้ว",
        "Temporary password set"
    ),
    (
        "kernel.admin.staff.temp_password",
        "รหัสผ่านชั่วคราว",
        "Temporary password"
    ),
    (
        "kernel.admin.staff.shown_once",
        "รหัสนี้แสดงครั้งเดียว กรุณาจดและส่งให้เจ้าหน้าที่ทางช่องทางที่ปลอดภัย",
        "This password is shown only once. Write it down and pass it on safely."
    ),
    ("kernel.admin.staff.open", "ไปที่บัญชีนี้", "Open the account"),
    (
        "kernel.admin.staff.roles_help",
        "สิทธิ์ของเจ้าหน้าที่คือสิทธิ์รวมของทุกบทบาทที่เลือก",
        "A Staff member holds every Permission of the Roles ticked."
    ),
    ("kernel.admin.staff.access", "การเข้าใช้", "Access"),
    (
        "kernel.admin.staff.failed_logins",
        "ใส่รหัสผ่านผิดติดกัน:",
        "Wrong passwords in a row:"
    ),
    ("kernel.admin.staff.unlock", "ปลดล็อก", "Clear lockout"),
    (
        "kernel.admin.staff.unlocked",
        "ปลดล็อกแล้ว",
        "Lockout cleared"
    ),
    (
        "kernel.admin.staff.reset",
        "ตั้งรหัสผ่านชั่วคราว",
        "Set a temporary password"
    ),
    (
        "kernel.admin.staff.reset_help",
        "รหัสเดิมจะใช้ไม่ได้ และเจ้าหน้าที่จะถูกออกจากระบบทุกเครื่อง",
        "The old password stops working and every session of this account ends."
    ),
    (
        "kernel.admin.staff.disable",
        "ปิดใช้งานบัญชี",
        "Disable the account"
    ),
    (
        "kernel.admin.staff.enable",
        "เปิดใช้งานบัญชี",
        "Enable the account"
    ),
    (
        "kernel.admin.staff.disable_help",
        "เจ้าหน้าที่จะถูกออกจากระบบทันทีและเข้าสู่ระบบไม่ได้",
        "The Staff member is signed out at once and cannot sign in."
    ),
    (
        "kernel.admin.staff.own_account",
        "นี่คือบัญชีของคุณ เปลี่ยนรหัสผ่านได้ที่หน้าเปลี่ยนรหัสผ่าน",
        "This is your account. Change your password from the password page."
    ),
    (
        "kernel.admin.staff.own_refused",
        "ทำกับบัญชีของตัวเองไม่ได้",
        "You cannot do this to your own account"
    ),
    (
        "kernel.admin.staff.last_manager",
        "ต้องมีเจ้าหน้าที่ที่ใช้งานได้และจัดการเจ้าหน้าที่ได้อย่างน้อยหนึ่งคน",
        "At least one active Staff member must keep the right to manage Staff"
    ),
    (
        "kernel.admin.staff.err_name",
        "กรุณากรอกชื่อที่แสดง",
        "Please enter a display name"
    ),
    (
        "kernel.admin.staff.err_email",
        "อีเมลไม่ถูกต้อง",
        "This email is not valid"
    ),
    (
        "kernel.admin.staff.err_taken",
        "อีเมลนี้มีบัญชีอยู่แล้ว",
        "This email already has an account"
    ),
    (
        "kernel.admin.staff.err_roles",
        "บทบาทที่เลือกถูกลบไปแล้ว โหลดหน้านี้ใหม่ แล้วเลือกบทบาทอีกครั้ง",
        "A Role ticked no longer exists. Reload the page and tick the Roles again."
    ),
    // Roles
    ("kernel.role.Administrator", "ผู้ดูแลระบบ", "Administrator"),
    ("kernel.admin.roles.group_kernel", "ระบบหลัก", "Core"),
    (
        "kernel.admin.roles.stored",
        "ชื่อบทบาทที่บันทึกไว้ในระบบ",
        "Role names as stored"
    ),
    ("kernel.admin.roles.title", "บทบาทและสิทธิ์", "Roles"),
    ("kernel.admin.roles.create", "สร้างบทบาท", "Create a Role"),
    ("kernel.admin.roles.created", "สร้างบทบาทแล้ว", "Role created"),
    ("kernel.admin.roles.name", "ชื่อบทบาท", "Role name"),
    (
        "kernel.admin.roles.holders",
        "เจ้าหน้าที่ที่มีบทบาทนี้:",
        "Staff holding it:"
    ),
    (
        "kernel.admin.roles.toml",
        "มาจากไฟล์ตั้งค่า",
        "from deployment.toml"
    ),
    (
        "kernel.admin.roles.toml_help",
        "บทบาทนี้มาจาก deployment.toml ระบบให้สิทธิ์ตามไฟล์เพียงครั้งเดียว การแก้ไขหรือลบที่นี่จึงคงอยู่ migrate ครั้งต่อไปจะไม่คืนสิทธิ์ที่เอาออกและไม่สร้างบทบาทที่ลบไปแล้วขึ้นมาใหม่ จะเพิ่มเฉพาะสิทธิ์ที่เพิ่งเพิ่มเข้าไปในไฟล์ (หากระบุ kernel.link.manage โดยไม่มี kernel.link.revoke จะได้ kernel.link.revoke ด้วย)",
        "This Role comes from deployment.toml, which grants each of its Permissions once. Edits here, and deleting it, stick: a later migrate grants only Permissions newly added to the file (a Role listing kernel.link.manage without kernel.link.revoke also gets kernel.link.revoke), never one removed here, and does not re-create a deleted Role."
    ),
    ("kernel.admin.roles.delete", "ลบบทบาท", "Delete the Role"),
    (
        "kernel.admin.roles.delete_help",
        "ลบได้เฉพาะบทบาทที่ไม่มีเจ้าหน้าที่ถืออยู่",
        "Only a Role nobody holds can be deleted."
    ),
    ("kernel.admin.roles.deleted", "ลบบทบาทแล้ว", "Role deleted"),
    (
        "kernel.admin.roles.in_use",
        "ยังมีเจ้าหน้าที่ถือบทบาทนี้อยู่ จึงลบไม่ได้",
        "Staff still hold this Role, so it cannot be deleted"
    ),
    (
        "kernel.admin.roles.err_name",
        "ชื่อบทบาทต้องมี 1 ถึง 64 ตัวอักษร",
        "A Role name is 1 to 64 characters"
    ),
    (
        "kernel.admin.roles.err_taken",
        "มีบทบาทชื่อนี้อยู่แล้ว",
        "A Role with this name already exists"
    ),
    (
        "kernel.admin.roles.undeclared",
        "สิทธิ์ที่ไม่มีโมดูลใดประกาศแล้วจะไม่แสดงที่นี่ และจะถูกนำออกเมื่อกดบันทึก",
        "Permissions no Module declares any more are not shown here and are removed when you save."
    ),
    (
        "kernel.admin.roles.err_permission",
        "มีสิทธิ์ที่ระบบไม่รู้จักแล้ว โหลดหน้านี้ใหม่ แล้วเลือกสิทธิ์อีกครั้ง",
        "A Permission is no longer known. Reload the page and tick the Permissions again."
    ),
];
