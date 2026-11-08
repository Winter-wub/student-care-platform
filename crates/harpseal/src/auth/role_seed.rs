//! `[roles]` seeded once (#110): each (Role, Permission) pair of the TOML is
//! granted by the first `migrate` that sees it, and each Role name created
//! by the first that sees it. `role_seed` remembers what was seeded, so a
//! later `migrate` grants only pairs it has never seeded: a Permission an
//! Administrator took away, or a Role they deleted, stays that way.
//!
//! On a database from before `role_seed`, the first `migrate` finds no row
//! there: it records every pair the database already holds without writing
//! it, and grants the others, exactly as every `migrate` before it did.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::staff_admin::gated;
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::kernel::{LINK_MANAGE, LINK_REVOKE};
use crate::port_types::{Statement, Value};
use crate::{Result, Services};

/// `role_seed.permission_id` of the row that records the Role itself.
const ROLE_ROW: &str = "";

/// What a TOML Role is granted for the Permissions it lists: a Role that
/// lists `kernel.link.manage` and not `kernel.link.revoke` was written when
/// the former covered both, so it gets both. A Role that lists the new one
/// (alone or with the old) is taken as written.
fn with_legacy_link_grant(perms: &[String]) -> Vec<String> {
    let mut all = perms.to_vec();
    let has = |id: &str| perms.iter().any(|p| p == id);
    if has(LINK_MANAGE) && !has(LINK_REVOKE) {
        all.push(LINK_REVOKE.to_string());
    }
    all
}

/// A Role of the database, as the seed sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Held {
    id: String,
    permissions: BTreeSet<String>,
}

/// What one `migrate` does for one TOML Role.
#[derive(Debug, Default, PartialEq, Eq)]
struct Step {
    /// The Role is created (its name was never seeded and no Role has it).
    create: bool,
    /// The Role name is recorded as seeded.
    record_role: bool,
    /// Pairs recorded as seeded: every listed Permission never seeded.
    record: Vec<String>,
    /// Of those, the ones the Role does not hold yet: really granted.
    grant: Vec<String>,
}

/// The seed's decision for Role `name` listing `wanted` (the legacy rule
/// applied), given what was seeded and the Role in the database, if any.
/// A Role whose name was seeded and that is gone was deleted on purpose:
/// it is not re-created, but its unseen pairs are still recorded, so a
/// Role an Administrator later creates under that name never inherits
/// Permissions added to the TOML meanwhile.
fn plan(
    name: &str,
    wanted: &[String],
    seeded: &BTreeSet<(String, String)>,
    held: Option<&Held>,
) -> Step {
    let seen = |p: &str| seeded.contains(&(name.to_string(), p.to_string()));
    let role_seen = seen(ROLE_ROW);
    let mut record: Vec<String> = wanted.iter().filter(|p| !seen(p)).cloned().collect();
    record.sort();
    record.dedup();
    if role_seen && held.is_none() {
        return Step {
            record,
            ..Step::default()
        };
    }
    let grant = record
        .iter()
        .filter(|p| !held.is_some_and(|h| h.permissions.contains(*p)))
        .cloned()
        .collect();
    Step {
        create: held.is_none(),
        record_role: !role_seen,
        record,
        grant,
    }
}

/// Every row of `role_seed`: the (Role name, Permission) pairs seeded.
async fn seeded(services: &Services) -> Result<BTreeSet<(String, String)>> {
    let rows = services
        .db
        .query(Statement::new(
            "SELECT role_name, permission_id FROM role_seed",
        ))
        .await?;
    let text = |i: usize, c: &str| {
        rows.get(i, c)
            .and_then(|v| v.as_text())
            .unwrap_or_default()
            .to_string()
    };
    Ok((0..rows.len())
        .map(|i| (text(i, "role_name"), text(i, "permission_id")))
        .collect())
}

/// Role `name` with its Permissions, if it exists.
async fn held(services: &Services, name: &str) -> Result<Option<Held>> {
    let rows = services
        .db
        .query(
            Statement::new(
                "SELECT r.id, rp.permission_id FROM role r \
                 LEFT JOIN role_permission rp ON rp.role_id = r.id WHERE r.name = ?",
            )
            .bind(name),
        )
        .await?;
    let Some(id) = rows.get(0, "id").and_then(|v| v.as_text()) else {
        return Ok(None);
    };
    let permissions = (0..rows.len())
        .filter_map(|i| rows.get(i, "permission_id").and_then(|v| v.as_text()))
        .map(str::to_string)
        .collect();
    Ok(Some(Held {
        id: id.to_string(),
        permissions,
    }))
}

/// `NOT EXISTS` a `role_seed` row for (name, permission), two params: a
/// grant guarded by it does nothing once another `migrate` (racing this
/// one) has seeded the pair, so it never re-grants what an Administrator
/// removed meanwhile.
const UNSEEDED: &str =
    "NOT EXISTS (SELECT 1 FROM role_seed WHERE role_name = ? AND permission_id = ?)";

/// Statements carrying out `step` for Role `name`, in batch order: the
/// Role, the grants, then the records (so the grants' [`UNSEEDED`] guards
/// see the table as it was).
///
/// When `audited` and the step creates or grants, the audit row comes first
/// (`role.create`, or `role.update` with the Permissions before and after;
/// actor `system`), written only if the Role (create) or every pair to
/// grant (update) is still unseeded and the Role still absent (create) or
/// present (update); every other statement of the step is gated on that
/// row. So the audit row and its writes land together or not at all: a
/// racing `migrate` that got there first leaves this step to nothing, and
/// what it did not record stays unseen for the next `migrate`.
fn statements(
    services: &Services,
    name: &str,
    step: &Step,
    held: Option<&Held>,
    audited: bool,
) -> Result<Vec<Statement>> {
    let now = services.now_ms();
    let id = match held {
        Some(h) => h.id.clone(),
        None => services.new_id()?,
    };
    let mut stmts = Vec::new();
    let mut gate = None;
    if audited && (step.create || !step.grant.is_empty()) {
        let (row, g) = audit_row(services, name, &id, step, held)?;
        stmts.push(row);
        gate = Some(g);
    }
    let write = |sql: String, params: Vec<Value>| match &gate {
        Some(g) => gated(&sql, params, g),
        None => {
            let mut st = Statement::new(sql);
            st.params = params;
            st
        }
    };
    if step.create {
        stmts.push(write(
            format!(
                "INSERT OR IGNORE INTO role (id, name, created_at) SELECT ?, ?, ? WHERE {UNSEEDED}"
            ),
            vec![
                id.as_str().into(),
                name.into(),
                now.into(),
                name.into(),
                ROLE_ROW.into(),
            ],
        ));
    }
    for p in &step.grant {
        stmts.push(write(
            format!(
                "INSERT OR IGNORE INTO role_permission (role_id, permission_id) \
                 SELECT id, ? FROM role WHERE name = ? AND {UNSEEDED}"
            ),
            vec![
                p.as_str().into(),
                name.into(),
                name.into(),
                p.as_str().into(),
            ],
        ));
    }
    let roles = step.record_role.then_some(ROLE_ROW);
    for p in roles
        .into_iter()
        .chain(step.record.iter().map(String::as_str))
    {
        stmts.push(write(
            "INSERT OR IGNORE INTO role_seed (role_name, permission_id, seeded_at) \
             SELECT ?, ?, ? WHERE 1 = 1"
                .to_string(),
            vec![name.into(), p.into(), now.into()],
        ));
    }
    Ok(stmts)
}

/// The conditional audit row of a step that creates or grants, and its id
/// (the gate). See [`statements`].
fn audit_row(
    services: &Services,
    name: &str,
    id: &str,
    step: &Step,
    held: Option<&Held>,
) -> Result<(Statement, String)> {
    let target = Target::new("role", id);
    let ctx = RequestCtx::default();
    if step.create {
        let cond = Statement::new(format!(
            "NOT EXISTS (SELECT 1 FROM role WHERE name = ?) AND {UNSEEDED}"
        ))
        .bind(name)
        .bind(name)
        .bind(ROLE_ROW);
        let after = json!({ "name": name, "permissions": step.grant });
        return audit::entry_if(
            services,
            &Actor::System,
            "role.create",
            Some(&target),
            None,
            Some(after),
            &ctx,
            cond,
        );
    }
    let marks = vec!["?"; step.grant.len()].join(", ");
    let mut cond = Statement::new(format!(
        "EXISTS (SELECT 1 FROM role WHERE id = ?) AND NOT EXISTS (SELECT 1 FROM role_seed \
         WHERE role_name = ? AND permission_id IN ({marks}))"
    ))
    .bind(id)
    .bind(name);
    for p in &step.grant {
        cond = cond.bind(p.as_str());
    }
    let before: BTreeSet<&String> = held
        .map(|h| h.permissions.iter().collect())
        .unwrap_or_default();
    let mut after = before.clone();
    after.extend(step.grant.iter());
    audit::entry_if(
        services,
        &Actor::System,
        "role.update",
        Some(&target),
        Some(json!({ "name": name, "permissions": before })),
        Some(json!({ "name": name, "permissions": after })),
        &ctx,
        cond,
    )
}

/// Seeds `[roles]` into `role` / `role_permission` once (see the module
/// docs): creates the Roles and grants the pairs never seeded, records them
/// in `role_seed`, and audits what it really granted. One batch.
///
/// The seed of a database with no Role and nothing in `role_seed` yet (a
/// new Deployment's first `migrate`) is its installation, not a change to
/// the Roles, and writes no audit row, as before `role_seed`; every later
/// grant is audited, including the first seed after every Role was
/// deleted (`role_seed` remembers them).
///
/// Returns one notice per Role the legacy rule widened, for the migrate
/// report: the Core has no logger, and the Deployment's author should list
/// what the Role means.
pub(crate) async fn seed(
    services: &Services,
    roles: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<String>> {
    let seeded = seeded(services).await?;
    let installing = seeded.is_empty()
        && services
            .db
            .query(Statement::new("SELECT 1 FROM role LIMIT 1"))
            .await?
            .is_empty();
    let mut stmts = Vec::new();
    let mut notices = Vec::new();
    for (name, perms) in roles {
        let wanted = with_legacy_link_grant(perms);
        if wanted.len() != perms.len() {
            notices.push(format!(
                "[roles] {name} lists {LINK_MANAGE} without {LINK_REVOKE}, so it is also \
                 granted {LINK_REVOKE}; list both, or {LINK_REVOKE} alone, to say what it means"
            ));
        }
        let held = held(services, name).await?;
        let step = plan(name, &wanted, &seeded, held.as_ref());
        stmts.extend(statements(
            services,
            name,
            &step,
            held.as_ref(),
            !installing,
        )?);
    }
    if !stmts.is_empty() {
        services.db.batch(stmts).await?;
    }
    Ok(notices)
}

#[cfg(test)]
#[path = "role_seed_tests.rs"]
mod tests;
