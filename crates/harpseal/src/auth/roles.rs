//! Roles: seeded once from `[roles]` in the TOML ([`super::role_seed`]),
//! then edited in the database by Administrators; Permissions are resolved
//! per request.

use crate::port_types::{Rows, Statement};
use crate::{Error, Result, Services};

/// Name of the Role the CLI gives the first Administrator.
pub(crate) const ADMINISTRATOR: &str = "Administrator";

/// Prefix of the catalog key holding a Role's display label (#125):
/// `kernel.role.<stored name>`, e.g. `kernel.role.Administrator`.
pub(crate) const LABEL_PREFIX: &str = "kernel.role.";

/// The name Staff read for Role `name`: its `kernel.role.<name>` text when
/// the catalog has one (the Kernel ships "ผู้ดูแลระบบ" for
/// [`ADMINISTRATOR`]; a Deployment adds its own in `[texts]`), else the
/// stored name. The stored name never changes, so no migration is needed.
pub(crate) fn label(catalog: &crate::Catalog, locale: &crate::Locale, name: &str) -> String {
    let key = format!("{LABEL_PREFIX}{name}");
    if catalog.knows(&key) {
        catalog.t(locale, &key)
    } else {
        name.to_string()
    }
}

/// Statements creating Role `name` if absent and granting `perms`, with
/// the id a new Role gets (an existing Role keeps its own).
pub(crate) fn ensure_role(
    services: &Services,
    name: &str,
    perms: &[impl AsRef<str>],
) -> Result<(String, Vec<Statement>)> {
    let id = services.new_id()?;
    let mut stmts = vec![
        Statement::new(
            "INSERT INTO role (id, name, created_at) VALUES (?, ?, ?) ON CONFLICT (name) DO NOTHING",
        )
        .bind(id.as_str())
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
    Ok((id, stmts))
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

/// Whether any of the Roles `names` grants Permission `permission`.
pub(crate) async fn grant_any(
    services: &Services,
    names: &[String],
    permission: &str,
) -> Result<bool> {
    for name in names {
        let rows = services
            .db
            .query(
                Statement::new(
                    "SELECT 1 FROM role_permission rp JOIN role r ON r.id = rp.role_id \
                     WHERE r.name = ? AND rp.permission_id = ?",
                )
                .bind(name.as_str())
                .bind(permission),
            )
            .await?;
        if !rows.is_empty() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// One Role with its Permissions and how many Staff hold it.
#[derive(Debug, Clone)]
pub(crate) struct RoleRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) permissions: Vec<String>,
    pub(crate) holders: i64,
}

impl RoleRow {
    /// The compare-and-set token of the edit form: the sorted Permissions.
    pub(crate) fn version(&self) -> String {
        self.permissions.join(",")
    }
}

const ROLE_SELECT: &str = "SELECT r.id, r.name, \
    (SELECT json_group_array(permission_id) FROM (SELECT permission_id FROM role_permission \
     WHERE role_id = r.id ORDER BY permission_id)) AS perms, \
    (SELECT count(*) FROM staff_role WHERE role_id = r.id) AS holders FROM role r";

/// Every Role, by name.
pub(crate) async fn list(services: &Services) -> Result<Vec<RoleRow>> {
    let rows = services
        .db
        .query(Statement::new(format!("{ROLE_SELECT} ORDER BY r.name")))
        .await?;
    Ok(role_rows(&rows))
}

/// One Role by id.
pub(crate) async fn get(services: &Services, id: &str) -> Result<Option<RoleRow>> {
    let rows = services
        .db
        .query(Statement::new(format!("{ROLE_SELECT} WHERE r.id = ?")).bind(id))
        .await?;
    Ok(role_rows(&rows).into_iter().next())
}

fn role_rows(rows: &Rows) -> Vec<RoleRow> {
    let text = |i: usize, c: &str| {
        rows.get(i, c)
            .and_then(|v| v.as_text())
            .unwrap_or_default()
            .to_string()
    };
    (0..rows.len())
        .map(|i| RoleRow {
            id: text(i, "id"),
            name: text(i, "name"),
            permissions: {
                // json_group_array keeps no guaranteed order.
                let mut p: Vec<String> =
                    serde_json::from_str(&text(i, "perms")).unwrap_or_default();
                p.sort();
                p
            },
            holders: rows.get(i, "holders").and_then(|v| v.as_i64()).unwrap_or(0),
        })
        .collect()
}

/// The public write functions (`roster::upload`, …) check what the page
/// guard checks: `actor` is an active Staff member holding `permission`.
/// Returns the Staff id; anything else is [`Error::Forbidden`], so a CLI
/// cannot bypass Roles.
pub(crate) async fn require(
    services: &Services,
    actor: &crate::Actor,
    permission: &str,
) -> Result<String> {
    let forbidden = || Error::Forbidden {
        permission: permission.to_string(),
    };
    let crate::Actor::Staff(id) = actor else {
        return Err(forbidden());
    };
    let rows = services
        .db
        .query(Statement::new("SELECT status FROM staff WHERE id = ?").bind(id.as_str()))
        .await?;
    let active = rows.get(0, "status").and_then(|v| v.as_text()) == Some("active");
    if !active
        || !permissions_of(services, id)
            .await?
            .iter()
            .any(|p| p == permission)
    {
        return Err(forbidden());
    }
    Ok(id.clone())
}
