//! The Kernel as a built-in Module: its Permissions, default texts,
//! migrations and public pages. Registered first by every Deployment. Not
//! public API: the schema and these queries may change without notice.

pub(crate) mod admin;
pub(crate) mod app;
pub mod audience;
pub(crate) mod flow;
pub mod links;
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

/// Permissions the Kernel declares (spec, "Back office").
pub(crate) const PERMISSIONS: &[Permission] = &[
    Permission::new(
        "kernel.student.view",
        "See Students, their Links, Tags and Consent",
    ),
    Permission::new(
        "kernel.student.edit",
        "Edit Student fields and Tags by hand",
    ),
    Permission::new("kernel.link.manage", "Create Manual Links and revoke Links"),
    Permission::new("kernel.roster.view", "See roster imports"),
    Permission::new("kernel.roster.edit", "Edit roster data"),
    Permission::new("kernel.roster.import", "Upload and apply roster files"),
    Permission::new("kernel.usage.view", "See Channel usage and Quotas"),
    Permission::new("kernel.security.view", "See Security Events"),
    Permission::new("kernel.audit.view", "See the audit log"),
    Permission::new("kernel.staff.manage", "Manage Staff accounts and Roles"),
    Permission::new(
        "kernel.setting.edit",
        "Edit texts and the privacy notice version",
    ),
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
