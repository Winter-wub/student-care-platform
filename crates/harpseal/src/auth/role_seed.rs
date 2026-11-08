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

use crate::audit::{self, Actor, RequestCtx, Target};
use crate::kernel::{LINK_MANAGE, LINK_REVOKE};
use crate::port_types::Statement;
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
/// nothing is done for it, and nothing recorded.
fn plan(
    name: &str,
    wanted: &[String],
    seeded: &BTreeSet<(String, String)>,
    held: Option<&Held>,
) -> Step {
    let seen = |p: &str| seeded.contains(&(name.to_string(), p.to_string()));
    let role_seen = seen(ROLE_ROW);
    if role_seen && held.is_none() {
        return Step::default();
    }
    let mut record: Vec<String> = wanted.iter().filter(|p| !seen(p)).cloned().collect();
    record.sort();
    record.dedup();
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

fn record(name: &str, permission: &str, now: i64) -> Statement {
    Statement::new(
        "INSERT OR IGNORE INTO role_seed (role_name, permission_id, seeded_at) VALUES (?, ?, ?)",
    )
    .bind(name)
    .bind(permission)
    .bind(now)
}

/// Statements carrying out `step` for Role `name`, with, when `audited`,
/// the audit row of what it really grants (`role.create`, or `role.update`
/// with the Permissions before and after; actor `system`). A step that
/// only records writes no audit row.
fn statements(
    services: &Services,
    name: &str,
    step: &Step,
    held: Option<&Held>,
    audited: bool,
) -> Result<Vec<Statement>> {
    let now = services.now_ms();
    let mut stmts = Vec::new();
    let id = match held {
        Some(h) => h.id.clone(),
        None => services.new_id()?,
    };
    if step.create {
        stmts.push(
            Statement::new(
                "INSERT INTO role (id, name, created_at) VALUES (?, ?, ?) \
                 ON CONFLICT (name) DO NOTHING",
            )
            .bind(id.as_str())
            .bind(name)
            .bind(now),
        );
    }
    for p in &step.grant {
        stmts.push(
            Statement::new(
                "INSERT OR IGNORE INTO role_permission (role_id, permission_id) \
                 SELECT id, ? FROM role WHERE name = ?",
            )
            .bind(p.as_str())
            .bind(name),
        );
    }
    if step.record_role {
        stmts.push(record(name, ROLE_ROW, now));
    }
    stmts.extend(step.record.iter().map(|p| record(name, p, now)));
    if !audited {
        return Ok(stmts);
    }
    let target = Target::new("role", id);
    let ctx = RequestCtx::default();
    if step.create {
        let after = json!({ "name": name, "permissions": step.grant });
        stmts.push(audit::entry(
            services,
            &Actor::System,
            "role.create",
            Some(&target),
            None,
            Some(after),
            &ctx,
        )?);
    } else if let (Some(h), false) = (held, step.grant.is_empty()) {
        let before: Vec<&String> = h.permissions.iter().collect();
        let mut after: BTreeSet<&String> = h.permissions.iter().collect();
        after.extend(step.grant.iter());
        stmts.push(audit::entry(
            services,
            &Actor::System,
            "role.update",
            Some(&target),
            Some(json!({ "name": name, "permissions": before })),
            Some(json!({ "name": name, "permissions": after })),
            &ctx,
        )?);
    }
    Ok(stmts)
}

/// Seeds `[roles]` into `role` / `role_permission` once (see the module
/// docs): creates the Roles and grants the pairs never seeded, records them
/// in `role_seed`, and audits what it really granted. One batch.
///
/// The seed of a database with no Role yet (a new Deployment's first
/// `migrate`) is its installation, not a change to the Roles, and writes no
/// audit row, as before `role_seed`; every later grant is audited.
///
/// Returns one notice per Role the legacy rule widened, for the migrate
/// report: the Core has no logger, and the Deployment's author should list
/// what the Role means.
pub(crate) async fn seed(
    services: &Services,
    roles: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<String>> {
    let seeded = seeded(services).await?;
    let installing = services
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
mod tests {
    use super::*;

    fn ids(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    fn seen(pairs: &[(&str, &str)]) -> BTreeSet<(String, String)> {
        pairs
            .iter()
            .map(|(r, p)| (r.to_string(), p.to_string()))
            .collect()
    }

    fn holding(p: &[&str]) -> Held {
        Held {
            id: "r1".into(),
            permissions: p.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_role_listing_only_the_old_link_permission_gets_both() {
        let got = with_legacy_link_grant(&ids(&["kernel.student.view", LINK_MANAGE]));
        assert_eq!(got, ids(&["kernel.student.view", LINK_MANAGE, LINK_REVOKE]));
    }

    #[test]
    fn a_role_listing_the_new_permission_is_taken_as_written() {
        let only_revoke = ids(&[LINK_REVOKE]);
        assert_eq!(with_legacy_link_grant(&only_revoke), only_revoke);
        let both = ids(&[LINK_MANAGE, LINK_REVOKE]);
        assert_eq!(with_legacy_link_grant(&both), both);
        let neither = ids(&["kernel.student.view"]);
        assert_eq!(with_legacy_link_grant(&neither), neither);
    }

    #[test]
    fn a_new_role_is_created_with_every_pair() {
        let step = plan("Reader", &ids(&["b", "a"]), &seen(&[]), None);
        assert!(step.create && step.record_role);
        assert_eq!(step.record, ids(&["a", "b"]));
        assert_eq!(step.grant, ids(&["a", "b"]));
    }

    #[test]
    fn a_seeded_pair_taken_away_is_not_granted_again() {
        let s = seen(&[("Reader", ""), ("Reader", "a"), ("Reader", "b")]);
        let step = plan("Reader", &ids(&["a", "b"]), &s, Some(&holding(&["b"])));
        assert_eq!(step, Step::default());
    }

    #[test]
    fn only_the_new_pair_is_granted() {
        let s = seen(&[("Reader", ""), ("Reader", "a")]);
        let step = plan("Reader", &ids(&["a", "c"]), &s, Some(&holding(&[])));
        assert!(!step.create && !step.record_role);
        assert_eq!(step.record, ids(&["c"]));
        assert_eq!(step.grant, ids(&["c"]));
    }

    #[test]
    fn an_existing_database_records_what_it_holds_and_grants_the_rest() {
        let step = plan(
            "Reader",
            &ids(&["a", "b"]),
            &seen(&[]),
            Some(&holding(&["a"])),
        );
        assert!(!step.create && step.record_role);
        assert_eq!(step.record, ids(&["a", "b"]));
        assert_eq!(
            step.grant,
            ids(&["b"]),
            "only what the old migrate added too"
        );
    }

    #[test]
    fn a_deleted_seeded_role_is_left_deleted() {
        let s = seen(&[("Reader", ""), ("Reader", "a")]);
        let step = plan("Reader", &ids(&["a", "new"]), &s, None);
        assert_eq!(step, Step::default());
    }
}
