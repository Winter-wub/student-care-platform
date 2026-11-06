//! Administrators' writes on Roles (the `/admin/roles` pages, #27):
//! create, edit Permissions, delete. Roles from `[roles]` live in the same
//! table (seeded add-only on every `migrate`), so they are edited the same
//! way; the next `migrate` grants their TOML Permissions again and
//! re-creates a deleted TOML Role.
//!
//! Each write is one `Db::batch` gated on its own audit row, like
//! [`super::staff_admin`].

use serde_json::json;

use super::roles::RoleRow;
use super::staff_admin::{MANAGE, Outcome, and, exists, gated};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::manifest::Permission;
use crate::port_types::Statement;
use crate::{Error, Result, Services};

/// Longest Role name, in characters.
pub(crate) const MAX_NAME_CHARS: usize = 64;

/// A Role name as stored, or `Error::Invalid`.
pub(crate) fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(Error::Invalid("role name".into()));
    }
    Ok(name.to_string())
}

/// `wanted` sorted and deduplicated, or `Error::Invalid` if any is not
/// in `declared`.
pub(crate) fn clean_permissions(declared: &[Permission], wanted: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = wanted.to_vec();
    out.sort();
    out.dedup();
    if out.iter().any(|p| !declared.iter().any(|d| d.id == p)) {
        return Err(Error::Invalid("unknown permission".into()));
    }
    Ok(out)
}

/// Creates Role `name` granting `permissions` (already cleaned). `Taken`
/// (as [`Outcome::Stale`]) when a Role has that name. Audited as
/// `role.create`; returns the new id.
pub(crate) async fn create(
    s: &Services,
    actor: &Actor,
    name: &str,
    permissions: &[String],
    ctx: &RequestCtx,
) -> Result<(Outcome, Option<String>)> {
    let id = s.new_id()?;
    let free = Statement::new("NOT EXISTS (SELECT 1 FROM role WHERE name = ?)").bind(name);
    let (row, gate) = audit::entry_if(
        s,
        actor,
        "role.create",
        Some(&Target::new("role", id.clone())),
        None,
        Some(json!({ "name": name, "permissions": permissions })),
        ctx,
        free,
    )?;
    let mut stmts = vec![
        row,
        gated(
            "INSERT INTO role (id, name, created_at) SELECT ?, ?, ? WHERE 1 = 1",
            vec![id.as_str().into(), name.into(), s.now_ms().into()],
            &gate,
        ),
    ];
    stmts.extend(permissions.iter().map(|p| grant(&id, p, &gate)));
    let done = s.db.batch(stmts).await?;
    Ok(if done.first().is_some_and(|r| r.rows_affected == 1) {
        (Outcome::Done, Some(id))
    } else {
        (Outcome::Stale, None)
    })
}

fn grant(role_id: &str, permission: &str, gate: &str) -> Statement {
    gated(
        "INSERT OR IGNORE INTO role_permission (role_id, permission_id) SELECT ?, ? WHERE 1 = 1",
        vec![role_id.into(), permission.into()],
        gate,
    )
}

/// The Permissions of `role_id` are still exactly `version` (the sorted,
/// comma-joined list the form showed), compared as a set: no reliance on
/// `group_concat` order.
fn same_permissions(role_id: &str, version: &str) -> Statement {
    let expected: Vec<&str> = version.split(',').filter(|p| !p.is_empty()).collect();
    let marks = vec!["?"; expected.len().max(1)].join(", ");
    let mut st = Statement::new(format!(
        "(SELECT count(*) FROM role_permission WHERE role_id = ?) = ? \
         AND NOT EXISTS (SELECT 1 FROM role_permission WHERE role_id = ? \
         AND permission_id NOT IN ({marks}))"
    ))
    .bind(role_id)
    .bind(expected.len() as i64)
    .bind(role_id);
    if expected.is_empty() {
        st = st.bind(crate::port_types::Value::Null);
    }
    for p in expected {
        st = st.bind(p);
    }
    st
}

/// `EXISTS`: an active Staff member holds `kernel.staff.manage` through a
/// Role other than `role_id`.
fn manager_elsewhere(role_id: &str) -> Statement {
    Statement::new(
        "EXISTS (SELECT 1 FROM staff o JOIN staff_role sr ON sr.staff_id = o.id \
         JOIN role_permission rp ON rp.role_id = sr.role_id \
         WHERE rp.permission_id = ? AND o.status = 'active' AND sr.role_id <> ?)",
    )
    .bind(MANAGE)
    .bind(role_id)
}

/// Replaces the Permissions of `role` with `permissions` (cleaned), if
/// they are still `version`. Taking `kernel.staff.manage` away is refused
/// when no active Staff member holds it through another Role. Audited as
/// `role.update`.
pub(crate) async fn set_permissions(
    s: &Services,
    actor: &Actor,
    role: &RoleRow,
    permissions: &[String],
    version: &str,
    ctx: &RequestCtx,
) -> Result<Outcome> {
    let mut cond = same_permissions(&role.id, version);
    let loses =
        role.permissions.iter().any(|p| p == MANAGE) && !permissions.iter().any(|p| p == MANAGE);
    if loses {
        if role.holders > 0 && !exists(s, manager_elsewhere(&role.id)).await? {
            return Ok(Outcome::LastManager);
        }
        // Holders may have appeared since the read: the gate decides.
        let elsewhere = manager_elsewhere(&role.id);
        let mut gate = Statement::new(format!(
            "(NOT EXISTS (SELECT 1 FROM staff_role WHERE role_id = ?) OR {})",
            elsewhere.sql
        ))
        .bind(role.id.as_str());
        gate.params.extend(elsewhere.params);
        cond = and(cond, gate);
    }
    let (row, gate) = audit::entry_if(
        s,
        actor,
        "role.update",
        Some(&Target::new("role", role.id.clone())),
        Some(json!({ "name": role.name, "permissions": role.permissions })),
        Some(json!({ "name": role.name, "permissions": permissions })),
        ctx,
        cond,
    )?;
    let mut stmts = vec![
        row,
        gated(
            "DELETE FROM role_permission WHERE role_id = ?",
            vec![role.id.as_str().into()],
            &gate,
        ),
    ];
    stmts.extend(permissions.iter().map(|p| grant(&role.id, p, &gate)));
    let done = s.db.batch(stmts).await?;
    Ok(if done.first().is_some_and(|r| r.rows_affected == 1) {
        Outcome::Done
    } else {
        Outcome::Stale
    })
}

/// Deletes `role` when no Staff member holds it (conditional, so a
/// concurrent grant wins). Audited as `role.delete`. [`Outcome::Stale`]
/// when someone holds it or it is gone.
pub(crate) async fn delete(
    s: &Services,
    actor: &Actor,
    role: &RoleRow,
    ctx: &RequestCtx,
) -> Result<Outcome> {
    let unused = Statement::new(
        "EXISTS (SELECT 1 FROM role WHERE id = ?) \
         AND NOT EXISTS (SELECT 1 FROM staff_role WHERE role_id = ?)",
    )
    .bind(role.id.as_str())
    .bind(role.id.as_str());
    let (row, gate) = audit::entry_if(
        s,
        actor,
        "role.delete",
        Some(&Target::new("role", role.id.clone())),
        Some(json!({ "name": role.name, "permissions": role.permissions })),
        None,
        ctx,
        unused,
    )?;
    let stmts = vec![
        row,
        gated(
            "DELETE FROM role_permission WHERE role_id = ?",
            vec![role.id.as_str().into()],
            &gate,
        ),
        gated(
            "DELETE FROM role WHERE id = ?",
            vec![role.id.as_str().into()],
            &gate,
        ),
    ];
    let done = s.db.batch(stmts).await?;
    Ok(if done.first().is_some_and(|r| r.rows_affected == 1) {
        Outcome::Done
    } else {
        Outcome::Stale
    })
}
