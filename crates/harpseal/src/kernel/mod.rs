//! The Kernel as a built-in Module: its Permissions, default texts,
//! migrations and public pages. Registered first by every Deployment. Not
//! public API: the schema and these queries may change without notice.

mod notice;
mod texts;

use axum::routing::get;

use crate::delivery::{ChannelAccountId, StudentId};
use crate::manifest::Permission;
use crate::message::Locale;
use crate::port_types::{Rows, Statement, Value};
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
        r.messages(texts::DEFAULTS)
            .public(notice::PATH, get(notice::page));
    }
}

/// The active Link of an account: the Student and their Locale.
pub(crate) fn find_link(account: &ChannelAccountId) -> Statement {
    Statement::new(
        "SELECT l.student_id, s.locale FROM channel_account ca \
         JOIN link l ON l.channel_account_id = ca.id AND l.status = 'active' \
         JOIN student s ON s.id = l.student_id \
         WHERE ca.channel = ? AND ca.external_id = ?",
    )
    .bind(account.channel.as_str())
    .bind(account.external_id.clone())
}

pub(crate) fn parse_link(rows: &Rows) -> Option<(StudentId, Locale)> {
    match rows.rows.first()?.as_slice() {
        [Value::Text(s), Value::Text(l)] => Some((StudentId(s.clone()), Locale(l.clone()))),
        _ => None,
    }
}
