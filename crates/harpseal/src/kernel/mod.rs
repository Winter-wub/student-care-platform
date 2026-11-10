//! The Kernel as a built-in Module: its Permissions, default texts,
//! migrations and public pages. Registered first by every Deployment. Not
//! public API: the schema and these queries may change without notice.

pub(crate) mod admin;
pub(crate) mod app;
pub mod audience;
pub(crate) mod branding;
pub(crate) mod flow;
pub(crate) mod home;
pub mod links;
pub(crate) mod menu;
pub(crate) mod notice;
pub(crate) mod rate_limit;
pub(crate) mod roster;
pub(crate) mod security;
pub mod students;
pub(crate) mod text;
mod texts;
mod texts_link;
pub(crate) mod usage;

use axum::routing::get;

use crate::manifest::Permission;
use crate::{Module, ModuleRegistrar};

/// Module name of the Kernel; its migrations are recorded under it.
pub(crate) const NAME: &str = "kernel";

/// Create a Manual Link. Before #80 it also covered revoking a Link.
pub(crate) const LINK_MANAGE: &str = "kernel.link.manage";
/// Revoke a Student's Link (Staff unlink), split off from [`LINK_MANAGE`].
pub(crate) const LINK_REVOKE: &str = "kernel.link.revoke";
/// Edit settings: the privacy notice version, branding, the LINE package.
/// Before #165 it also covered the Texts page.
pub(crate) const SETTING_EDIT: &str = "kernel.setting.edit";
/// Edit texts (the Texts page and `texts::set_override`), split off from
/// [`SETTING_EDIT`] (#165).
pub(crate) const TEXTS_EDIT: &str = "kernel.texts.edit";

/// Permissions the Kernel declares (spec, "Back office").
pub(crate) const PERMISSIONS: &[Permission] = &[
    Permission::new(
        "kernel.student.view",
        "See Students, their Links, Tags and Consent",
    )
    .thai("ดูข้อมูลนักศึกษา การผูกบัญชี ป้าย และความยินยอม"),
    Permission::new(
        "kernel.student.edit",
        "Edit Student fields and Tags by hand",
    )
    .thai("แก้ไขข้อมูลและป้ายของนักศึกษา"),
    Permission::new(LINK_MANAGE, "Create Manual Links").thai("ผูกบัญชีแชทให้นักศึกษาโดยเจ้าหน้าที่"),
    Permission::new(LINK_REVOKE, "Revoke a Student's Link (Staff unlink)")
        .thai("ยกเลิกการผูกบัญชีของนักศึกษา"),
    Permission::new("kernel.roster.view", "See roster imports").thai("ดูประวัติการนำเข้ารายชื่อ"),
    Permission::new("kernel.roster.edit", "Edit roster data").thai("แก้ไขข้อมูลรายชื่อนักศึกษา"),
    Permission::new("kernel.roster.import", "Upload and apply roster files")
        .thai("นำเข้ารายชื่อนักศึกษา"),
    Permission::new("kernel.usage.view", "See Channel usage and Quotas")
        .thai("ดูการใช้งานและโควตาข้อความ"),
    Permission::new("kernel.security.view", "See Security Events").thai("ดูเหตุการณ์ความปลอดภัย"),
    Permission::new("kernel.audit.view", "See the audit log").thai("ดูบันทึกการใช้งาน"),
    Permission::new("kernel.staff.manage", "Manage Staff accounts and Roles")
        .thai("จัดการบัญชีเจ้าหน้าที่และบทบาท"),
    Permission::new(
        SETTING_EDIT,
        "Edit the privacy notice version, branding and the LINE package",
    )
    .thai("แก้ไขเวอร์ชันประกาศความเป็นส่วนตัว แบรนด์และรูปเมนู และแพ็กเกจ LINE"),
    Permission::new(TEXTS_EDIT, "Edit texts on the Texts page")
        .thai("แก้ไขข้อความที่ระบบใช้ในแชทและหน้าต่าง ๆ"),
    Permission::new("kernel.backup.manage", "See the backups and back up now")
        .thai("ดูชุดสำรองข้อมูลและสั่งสำรองทันที"),
    Permission::new(
        "kernel.backup.download",
        "Download a backup or all data (Administrator-equivalent: backups hold password hashes)",
    )
    .thai("ดาวน์โหลดชุดสำรองหรือข้อมูลทั้งหมด (เทียบเท่าผู้ดูแลระบบ เพราะชุดสำรองมีรหัสผ่านที่เข้ารหัสไว้)"),
];

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_people",
        include_str!("../../migrations/0001_people.sql"),
    ),
    (
        "0002_channels",
        include_str!("../../migrations/0002_channels.sql"),
    ),
    (
        "0003_operations",
        include_str!("../../migrations/0003_operations.sql"),
    ),
    (
        "0004_inbound_file_error",
        include_str!("../../migrations/0004_inbound_file_error.sql"),
    ),
    (
        "0005_job_unique_key",
        include_str!("../../migrations/0005_job_unique_key.sql"),
    ),
    (
        "0006_inbound_event_id",
        include_str!("../../migrations/0006_inbound_event_id.sql"),
    ),
    (
        "0007_rate_limit",
        include_str!("../../migrations/0007_rate_limit.sql"),
    ),
    (
        "0008_app_session_expires_idx",
        include_str!("../../migrations/0008_app_session_expires_idx.sql"),
    ),
    (
        "0009_inbound_event_done",
        include_str!("../../migrations/0009_inbound_event_done.sql"),
    ),
    (
        "0010_outbound_message_index",
        include_str!("../../migrations/0010_outbound_message_index.sql"),
    ),
    (
        "0011_menu_revision",
        include_str!("../../migrations/0011_menu_revision.sql"),
    ),
    (
        "0012_link_revoke_permission",
        include_str!("../../migrations/0012_link_revoke_permission.sql"),
    ),
    (
        "0013_backup_run",
        include_str!("../../migrations/0013_backup_run.sql"),
    ),
    (
        "0014_module_upload",
        include_str!("../../migrations/0014_module_upload.sql"),
    ),
    (
        "0015_notify_delivery",
        include_str!("../../migrations/0015_notify_delivery.sql"),
    ),
    (
        "0016_student_email_key_idx",
        include_str!("../../migrations/0016_student_email_key_idx.sql"),
    ),
    (
        "0017_role_seed",
        include_str!("../../migrations/0017_role_seed.sql"),
    ),
    (
        "0018_link_picker_idx",
        include_str!("../../migrations/0018_link_picker_idx.sql"),
    ),
    (
        "0019_texts_edit_permission",
        include_str!("../../migrations/0019_texts_edit_permission.sql"),
    ),
];

/// The Kernel's own registrations.
pub(crate) struct Kernel;

impl Module for Kernel {
    fn name(&self) -> &'static str {
        NAME
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        for p in PERMISSIONS {
            r.permission(*p);
        }
        for (name, sql) in MIGRATIONS {
            r.migration(name, sql);
        }
        students::register(r);
        roster::register(r);
        admin::register(r);
        usage::register(r);
        app::register(r);
        menu::register(r);
        branding::register(r);
        home::register(r);
        r.messages(texts::DEFAULTS)
            .messages(texts_link::DEFAULTS)
            .public(notice::PATH, get(notice::page))
            .back_office(
                "kernel.audit.view",
                crate::jobs::status::PATH,
                get(crate::jobs::status::page),
            );
    }
}
