//! The administration pages of the back office (#27): Security Events,
//! the audit log, Staff, Roles and Texts.
//!
//! | Route | Permission |
//! |---|---|
//! | `GET /admin/security` (filters: kind, Student, dates; HTMX fragment) | `kernel.security.view` |
//! | `GET /admin/audit`, `GET /admin/audit/{id}` | `kernel.audit.view` |
//! | `GET /admin/staff`, `GET/POST /admin/staff/new`, `GET /admin/staff/{id}`, `POST …/{roles,status,password,unlock}` | `kernel.staff.manage` |
//! | `GET/POST /admin/roles`, `GET/POST /admin/roles/{id}`, `POST …/delete` | `kernel.staff.manage` |
//! | `GET /admin/texts`, `GET/POST /admin/texts/{key}`, `POST /admin/texts/consent` | `kernel.setting.edit` |
//! | `GET/POST /admin/settings` (Module settings, #75; see [`module_settings`]) | each setting's own Permission |
//! | `GET /admin/backups`, `POST …/now`, `GET …/{id}/download`, `GET …/export.xlsx` | `kernel.backup.manage` ([`backups`]) |
//!
//! Reads of Security Events and the audit log are logged as sensitive
//! reads (`security.view`, `audit.view`, `audit.detail`), one row per
//! request. Every write is one batch gated on its audit row; a stale form
//! is refused with a flash, never written.

mod audit;
mod audit_detail;
mod audit_words;
pub(crate) mod backups;
mod module_settings;
mod module_settings_texts;
mod roles_pages;
mod security;
mod settings;
mod staff_detail;
mod staff_pages;
mod texts;
pub mod texts_api;
mod texts_areas;
mod texts_audit;
mod texts_list;
mod texts_pages;
mod texts_staff;
mod view;

use axum::routing::{get, post};

use crate::module::ModuleRegistrar;

pub(crate) use backups::backup_schedule;
pub(crate) use module_settings::nav as settings_nav;
pub(crate) use module_settings::write as write_setting;
/// The audited `setting` row write the Kernel's own pages share (#115).
pub(crate) use settings::{reset as reset_setting_row, save as save_setting_row};

/// The Module settings page's handlers, mounted by `Deployment::router`
/// for any signed-in Staff member (the page checks each setting's
/// Permission).
pub(crate) fn settings_route() -> axum::routing::MethodRouter<crate::AppState> {
    get(module_settings::page).post(module_settings::submit)
}

/// Registers the pages on the Kernel, after Students (10) and the roster.
pub(crate) fn register(r: &mut ModuleRegistrar) {
    const SECURITY: &str = "kernel.security.view";
    const AUDIT: &str = "kernel.audit.view";
    const STAFF: &str = "kernel.staff.manage";
    const SETTING: &str = "kernel.setting.edit";
    backups::register(r);
    r.messages(texts::DEFAULTS)
        .messages(texts::STAFF)
        .messages(texts_audit::AUDIT)
        .messages(module_settings_texts::TEXTS)
        .back_office_page_in(
            crate::NavGroup::Admin,
            SECURITY,
            "/security",
            "kernel.admin.security.title",
            40,
            get(security::page),
        )
        .back_office_page_in(
            crate::NavGroup::Admin,
            AUDIT,
            "/audit",
            "kernel.admin.audit.title",
            50,
            get(audit::page),
        )
        .back_office(AUDIT, "/audit/{id}", get(audit_detail::page))
        .back_office_page_in(
            crate::NavGroup::Admin,
            STAFF,
            "/staff",
            "kernel.admin.staff.title",
            60,
            get(staff_pages::list),
        )
        .back_office(
            STAFF,
            "/staff/new",
            get(staff_pages::new_form).post(staff_pages::create),
        )
        .back_office(STAFF, "/staff/{id}", get(staff_detail::page))
        .back_office(STAFF, "/staff/{id}/roles", post(staff_detail::set_roles))
        .back_office(STAFF, "/staff/{id}/status", post(staff_detail::set_status))
        .back_office(
            STAFF,
            "/staff/{id}/password",
            post(staff_detail::reset_password),
        )
        .back_office(STAFF, "/staff/{id}/unlock", post(staff_detail::unlock))
        .back_office_page_in(
            crate::NavGroup::Admin,
            STAFF,
            "/roles",
            "kernel.admin.roles.title",
            70,
            get(roles_pages::list).post(roles_pages::create),
        )
        .back_office(
            STAFF,
            "/roles/{id}",
            get(roles_pages::edit).post(roles_pages::update),
        )
        .back_office(STAFF, "/roles/{id}/delete", post(roles_pages::delete))
        .back_office_page_in(
            crate::NavGroup::Admin,
            SETTING,
            "/texts",
            "kernel.admin.texts.title",
            80,
            get(texts_list::list),
        )
        .back_office(SETTING, "/texts/consent", post(texts_pages::consent))
        .back_office(
            SETTING,
            "/texts/{key}",
            get(texts_pages::edit).post(texts_pages::submit),
        );
}
