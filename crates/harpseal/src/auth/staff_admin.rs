//! Administrators' writes on Staff accounts (the `/admin/staff` pages,
//! #27): disable / enable, Roles, temporary password, clear lockout.
//!
//! Each is one `Db::batch` gated on its own audit row
//! ([`crate::audit::entry_if`]): the row is written only if the Staff
//! account still has the `updated_at` the page read (a compare-and-set,
//! bumped by every write here) and, where it applies, another active Staff
//! member still holds `kernel.staff.manage`; the change runs only if the
//! audit row exists. Never a password or hash in the audit row.

use serde_json::json;

use super::{roles, staff};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::port_types::{Rows, Statement, Value};
use crate::{Error, Result, Services};

/// The Permission that manages Staff and Roles.
pub(crate) const MANAGE: &str = "kernel.staff.manage";

/// One Staff account as the pages show it.
#[derive(Debug, Clone)]
pub(crate) struct StaffRow {
    pub(crate) id: String,
    pub(crate) email: String,
    pub(crate) display_name: String,
    pub(crate) status: String,
    pub(crate) must_change_password: bool,
    pub(crate) failed_logins: i64,
    pub(crate) locked_until: Option<i64>,
    pub(crate) updated_at: i64,
    pub(crate) roles: Vec<String>,
}

/// What a write did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Written.
    Done,
    /// The account changed since the page was read (or is gone).
    Stale,
    /// Refused: no other active Staff member would hold `kernel.staff.manage`.
    LastManager,
    /// Refused: an Administrator cannot do this to themself.
    OwnAccount,
}

const SELECT: &str = "SELECT s.id, s.email, s.display_name, s.status, s.must_change_password, \
    s.failed_logins, s.locked_until, s.updated_at, \
    (SELECT json_group_array(name) FROM (SELECT r.name FROM staff_role sr JOIN role r \
     ON r.id = sr.role_id WHERE sr.staff_id = s.id ORDER BY r.name)) AS roles FROM staff s";

fn rows_to_staff(rows: &Rows) -> Vec<StaffRow> {
    let text = |i: usize, c: &str| {
        rows.get(i, c)
            .and_then(Value::as_text)
            .unwrap_or_default()
            .to_string()
    };
    let int = |i: usize, c: &str| rows.get(i, c).and_then(Value::as_i64);
    (0..rows.len())
        .map(|i| StaffRow {
            id: text(i, "id"),
            email: text(i, "email"),
            display_name: text(i, "display_name"),
            status: text(i, "status"),
            must_change_password: int(i, "must_change_password") == Some(1),
            failed_logins: int(i, "failed_logins").unwrap_or(0),
            locked_until: int(i, "locked_until"),
            updated_at: int(i, "updated_at").unwrap_or(0),
            roles: serde_json::from_str(&text(i, "roles")).unwrap_or_default(),
        })
        .collect()
}

/// Every Staff account, by email.
pub(crate) async fn list(s: &Services) -> Result<Vec<StaffRow>> {
    let rows =
        s.db.query(Statement::new(format!("{SELECT} ORDER BY s.email")))
            .await?;
    Ok(rows_to_staff(&rows))
}

/// One Staff account.
pub(crate) async fn get(s: &Services, id: &str) -> Result<Option<StaffRow>> {
    let rows =
        s.db.query(Statement::new(format!("{SELECT} WHERE s.id = ?")).bind(id))
            .await?;
    Ok(rows_to_staff(&rows).into_iter().next())
}

/// `EXISTS`: an active Staff member other than `staff_id` holds
/// `kernel.staff.manage`.
pub(crate) fn other_manager(staff_id: &str) -> Statement {
    Statement::new(
        "EXISTS (SELECT 1 FROM staff o JOIN staff_role sr ON sr.staff_id = o.id \
         JOIN role_permission rp ON rp.role_id = sr.role_id \
         WHERE rp.permission_id = ? AND o.status = 'active' AND o.id <> ?)",
    )
    .bind(MANAGE)
    .bind(staff_id)
}

/// `cond AND cond2`, params in order.
pub(crate) fn and(a: Statement, b: Statement) -> Statement {
    let mut st = Statement::new(format!("{} AND {}", a.sql, b.sql));
    st.params = a.params;
    st.params.extend(b.params);
    st
}

fn unchanged(staff_id: &str, version: i64) -> Statement {
    Statement::new("EXISTS (SELECT 1 FROM staff WHERE id = ? AND updated_at = ?)")
        .bind(staff_id)
        .bind(version)
}

/// `sql` (ending in a `WHERE` clause) run only if audit row `gate` exists.
pub(crate) fn gated(sql: &str, params: Vec<Value>, gate: &str) -> Statement {
    let mut st = Statement::new(format!(
        "{sql} AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)"
    ));
    st.params = params;
    st.bind(gate)
}

fn bump(s: &Services, staff_id: &str, gate: &str) -> Statement {
    gated(
        "UPDATE staff SET updated_at = max(?, updated_at + 1) WHERE id = ?",
        vec![s.now_ms().into(), staff_id.into()],
        gate,
    )
}

/// Ends every session of `staff_id`, gated.
fn end_sessions(staff_id: &str, gate: &str) -> Statement {
    gated(
        "DELETE FROM staff_session WHERE staff_id = ?",
        vec![staff_id.into()],
        gate,
    )
}

/// Runs `stmts` (audit row first); `Done` if the audit row was written.
async fn run(s: &Services, stmts: Vec<Statement>) -> Result<bool> {
    let done = s.db.batch(stmts).await?;
    Ok(done.first().is_some_and(|r| r.rows_affected == 1))
}

/// Disables (`active = false`) or enables a Staff account. Disabling ends
/// every session of that account in the same batch, and is refused for
/// one's own account and for the last active holder of
/// `kernel.staff.manage`. Audited as `staff.disable` / `staff.enable`.
pub(crate) async fn set_active(
    s: &Services,
    actor: &Actor,
    staff_id: &str,
    active: bool,
    version: i64,
    ctx: &RequestCtx,
) -> Result<Outcome> {
    if !active && matches!(actor, Actor::Staff(me) if me == staff_id) {
        return Ok(Outcome::OwnAccount);
    }
    let (from, to) = if active {
        ("disabled", "active")
    } else {
        ("active", "disabled")
    };
    let mut cond = unchanged(staff_id, version);
    if !active {
        if !exists(s, other_manager(staff_id)).await? {
            return Ok(Outcome::LastManager);
        }
        cond = and(cond, other_manager(staff_id));
    }
    let action = if active {
        "staff.enable"
    } else {
        "staff.disable"
    };
    let (row, gate) = audit::entry_if(
        s,
        actor,
        action,
        Some(&Target::new("staff", staff_id)),
        Some(json!({ "status": from })),
        Some(json!({ "status": to, "sessions_ended": !active })),
        ctx,
        cond,
    )?;
    let mut stmts = vec![
        row,
        gated(
            "UPDATE staff SET status = ?, updated_at = max(?, updated_at + 1) WHERE id = ?",
            vec![to.into(), s.now_ms().into(), staff_id.into()],
            &gate,
        ),
    ];
    if !active {
        stmts.push(end_sessions(staff_id, &gate));
    }
    Ok(if run(s, stmts).await? {
        Outcome::Done
    } else {
        Outcome::Stale
    })
}

/// Whether `cond` (an `EXISTS (..)` expression) holds now.
pub(crate) async fn exists(s: &Services, cond: Statement) -> Result<bool> {
    let mut q = Statement::new(format!("SELECT {} AS ok", cond.sql));
    q.params = cond.params;
    let rows = s.db.query(q).await?;
    Ok(rows.get(0, "ok").and_then(Value::as_i64) == Some(1))
}

/// Replaces the Roles of `staff_id` with `names` (sorted, deduplicated).
/// Refused when the account would lose `kernel.staff.manage` and no other
/// active Staff member holds it. Audited as `staff.roles`.
pub(crate) async fn set_roles(
    s: &Services,
    actor: &Actor,
    staff_id: &str,
    names: &[String],
    version: i64,
    ctx: &RequestCtx,
) -> Result<Outcome> {
    let mut after: Vec<String> = names.to_vec();
    after.sort();
    after.dedup();
    roles::check_exist(s, &after).await?;
    let Some(cur) = get(s, staff_id).await? else {
        return Ok(Outcome::Stale);
    };
    let mut cond = unchanged(staff_id, version);
    if !roles::grant_any(s, &after, MANAGE).await? {
        if !exists(s, other_manager(staff_id)).await? {
            return Ok(Outcome::LastManager);
        }
        cond = and(cond, other_manager(staff_id));
    }
    let (row, gate) = audit::entry_if(
        s,
        actor,
        "staff.roles",
        Some(&Target::new("staff", staff_id)),
        Some(json!({ "roles": cur.roles })),
        Some(json!({ "roles": after })),
        ctx,
        cond,
    )?;
    let mut stmts = vec![
        row,
        gated(
            "DELETE FROM staff_role WHERE staff_id = ?",
            vec![staff_id.into()],
            &gate,
        ),
    ];
    for name in &after {
        stmts.push(gated(
            "INSERT OR IGNORE INTO staff_role (staff_id, role_id) \
             SELECT ?, id FROM role WHERE name = ?",
            vec![staff_id.into(), name.as_str().into()],
            &gate,
        ));
    }
    stmts.push(bump(s, staff_id, &gate));
    Ok(if run(s, stmts).await? {
        Outcome::Done
    } else {
        Outcome::Stale
    })
}

/// Sets a generated temporary password on `staff_id` (returned, to show
/// once): it must be changed at the next sign-in; the lockout is cleared
/// and every session of that account ends. Refused for one's own account
/// (use the change form). Audited as `staff.password_reset`.
pub(crate) async fn reset_password(
    s: &Services,
    actor: &Actor,
    staff_id: &str,
    version: i64,
    ctx: &RequestCtx,
) -> Result<(Outcome, Option<String>)> {
    if matches!(actor, Actor::Staff(me) if me == staff_id) {
        return Ok((Outcome::OwnAccount, None));
    }
    let password = temporary_password(s)?;
    let (row, gate) = audit::entry_if(
        s,
        actor,
        "staff.password_reset",
        Some(&Target::new("staff", staff_id)),
        None,
        Some(json!({ "must_change_password": true, "sessions_ended": true })),
        ctx,
        unchanged(staff_id, version),
    )?;
    let stmts = vec![
        row,
        gated(
            "UPDATE staff SET password_hash = ?, must_change_password = 1, failed_logins = 0, \
             locked_until = NULL, updated_at = max(?, updated_at + 1) WHERE id = ?",
            vec![
                staff::hash_password(s, &password)?.into(),
                s.now_ms().into(),
                staff_id.into(),
            ],
            &gate,
        ),
        end_sessions(staff_id, &gate),
    ];
    Ok(if run(s, stmts).await? {
        (Outcome::Done, Some(password))
    } else {
        (Outcome::Stale, None)
    })
}

/// Clears `failed_logins` and `locked_until`. Audited as `staff.unlock`.
pub(crate) async fn unlock(
    s: &Services,
    actor: &Actor,
    staff_id: &str,
    version: i64,
    ctx: &RequestCtx,
) -> Result<Outcome> {
    let Some(cur) = get(s, staff_id).await? else {
        return Ok(Outcome::Stale);
    };
    let (row, gate) = audit::entry_if(
        s,
        actor,
        "staff.unlock",
        Some(&Target::new("staff", staff_id)),
        Some(json!({ "failed_logins": cur.failed_logins, "locked_until": cur.locked_until })),
        Some(json!({ "failed_logins": 0, "locked_until": null })),
        ctx,
        unchanged(staff_id, version),
    )?;
    let stmts = vec![
        row,
        gated(
            "UPDATE staff SET failed_logins = 0, locked_until = NULL, updated_at = max(?, updated_at + 1) WHERE id = ?",
            vec![s.now_ms().into(), staff_id.into()],
            &gate,
        ),
    ];
    Ok(if run(s, stmts).await? {
        Outcome::Done
    } else {
        Outcome::Stale
    })
}

/// A temporary password: 18 lowercase hex characters (72 random bits),
/// grouped by 6 for reading aloud.
pub(crate) fn temporary_password(s: &Services) -> Result<String> {
    let hex = s.random_hex(9)?;
    let groups: Vec<&str> = (0..3).map(|i| &hex[i * 6..i * 6 + 6]).collect();
    let p = groups.join("-");
    staff::check_password_rules(&p).map_err(|_| Error::Invalid("temporary password".into()))?;
    Ok(p)
}
