//! Roles: seeded from `[roles]` in the TOML on every migrate, extended in
//! the database by Administrators; Permissions are resolved per request.

use std::collections::BTreeMap;

use crate::port_types::{Rows, Statement};
use crate::{Error, Result, Services};

/// Name of the Role the CLI gives the first Administrator.
pub(crate) const ADMINISTRATOR: &str = "Administrator";

/// Merges `[roles]` into `role` / `role_permission`: missing Roles are
/// created and every listed Permission is granted. Add-only and idempotent:
/// Permissions granted in the database, and Roles created there, are kept.
pub(crate) async fn seed(services: &Services, roles: &BTreeMap<String, Vec<String>>) -> Result<()> {
    let mut stmts = Vec::new();
    for (name, perms) in roles {
        stmts.extend(ensure_role(services, name, perms)?);
    }
    if stmts.is_empty() {
        return Ok(());
    }
    services.db.batch(stmts).await.map(|_| ())
}

/// Statements creating Role `name` if absent and granting `perms`.
pub(crate) fn ensure_role(
    services: &Services,
    name: &str,
    perms: &[impl AsRef<str>],
) -> Result<Vec<Statement>> {
    let mut stmts = vec![
        Statement::new(
            "INSERT INTO role (id, name, created_at) VALUES (?, ?, ?) ON CONFLICT (name) DO NOTHING",
        )
        .bind(services.new_id()?)
        .bind(name)
        .bind(services.now_ms()),
    ];
    for p in perms {
        stmts.push(
            Statement::new(
                "INSERT OR IGNORE INTO role_permission (role_id, permission_id) \
                 SELECT id, ? FROM role WHERE name = ?",
            )
            .bind(p.as_ref())
            .bind(name),
        );
    }
    Ok(stmts)
}

fn column(rows: &Rows) -> Vec<String> {
    rows.rows
        .iter()
        .filter_map(|r| r.first().and_then(|v| v.as_text()).map(str::to_string))
        .collect()
}

/// Every Permission id `staff_id` holds through its Roles.
pub(crate) async fn permissions_of(services: &Services, staff_id: &str) -> Result<Vec<String>> {
    let rows = services
        .db
        .query(
            Statement::new(
                "SELECT DISTINCT rp.permission_id FROM staff_role sr \
                 JOIN role_permission rp ON rp.role_id = sr.role_id WHERE sr.staff_id = ? \
                 ORDER BY rp.permission_id",
            )
            .bind(staff_id),
        )
        .await?;
    Ok(column(&rows))
}

/// The names of `staff_id`'s Roles, sorted.
pub(crate) async fn roles_of(services: &Services, staff_id: &str) -> Result<Vec<String>> {
    let rows = services
        .db
        .query(
            Statement::new(
                "SELECT r.name FROM staff_role sr JOIN role r ON r.id = sr.role_id \
                 WHERE sr.staff_id = ? ORDER BY r.name",
            )
            .bind(staff_id),
        )
        .await?;
    Ok(column(&rows))
}

/// Refuses any name that is not a Role in the database.
pub(crate) async fn check_exist(services: &Services, names: &[String]) -> Result<()> {
    for name in names {
        let rows = services
            .db
            .query(Statement::new("SELECT id FROM role WHERE name = ?").bind(name.as_str()))
            .await?;
        if rows.is_empty() {
            return Err(Error::Invalid(format!("no role named `{name}`")));
        }
    }
    Ok(())
}

/// Grants Role `name` to `staff_id`, for a batch.
pub(crate) fn grant(staff_id: &str, name: &str) -> Statement {
    Statement::new(
        "INSERT OR IGNORE INTO staff_role (staff_id, role_id) SELECT ?, id FROM role WHERE name = ?",
    )
    .bind(staff_id)
    .bind(name)
}
