//! Staff writes on a Student. Each is one `Db::batch` whose first
//! statement is its audit row, written only if the Student still holds
//! what was read; the change itself runs only if that row exists.

use serde_json::{Map, Value as Json, json};

use super::read::{self, StudentRow};
use super::{CORE_OVERRIDES, StudentFields, attribute_field};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::deployment::Deployment;
use crate::kernel::links;
use crate::port_types::{ExecResult, Statement};
use crate::{Error, Result, Services};

pub(super) async fn load(s: &Services, id: &str) -> Result<StudentRow> {
    read::student(s, id)
        .await?
        .ok_or_else(|| Error::Invalid(format!("no student {id}")))
}

pub(super) fn target(id: &str) -> Target {
    Target::new("student", id)
}

/// `sql` (ending in a `WHERE` clause) with `params`, run only if the audit
/// row `gate` exists; the gate is bound last.
pub(super) fn gated_stmt(sql: &str, gate: &str, params: Vec<crate::Value>) -> Statement {
    let sql = format!("{sql} AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)");
    let mut st = Statement::new(sql);
    st.params = params;
    st.bind(gate)
}

pub(super) fn affected(done: &[ExecResult], i: usize) -> u64 {
    done.get(i).map_or(0, |r| r.rows_affected)
}

fn changed_elsewhere() -> Error {
    Error::Conflict("the Student changed since the page was read".into())
}

/// Saves `fields` over Student `student_id` as a manual edit: every field
/// whose value changes is added to `manual_overrides` (so imports keep
/// it), and the audit row `student.update` holds the changed fields'
/// values before and after (plus the override list). Returns the changed
/// fields (`display_name`, `email`, `locale`, `attributes.<key>`); none
/// means nothing was written. `fields` is not validated here: the edit
/// form checks it against the `[roster.attributes]` schema. The status is
/// not changed here ([`set_status`] revokes Links).
///
/// `version` is the token of the Student the caller started from (the edit
/// form's hidden `version` field): when given and the Student no longer
/// matches it, nothing is written, so fields the caller did not touch are
/// never written back stale. `None` skips that check (the write is still a
/// compare-and-set against this function's own read).
///
/// `Err(Error::Invalid)` for an unknown Student or a status change,
/// `Err(Error::Conflict)` when the Student changed since `version` or
/// since it was read.
pub async fn update(
    d: &Deployment,
    student_id: &str,
    fields: &StudentFields,
    version: Option<&str>,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<Vec<String>> {
    let s = d.services();
    let cur = load(s, student_id).await?;
    if version.is_some_and(|v| v != cur.version()) {
        return Err(changed_elsewhere());
    }
    if fields.status != cur.status {
        return Err(Error::Invalid(
            "the status changes through set_status".into(),
        ));
    }
    let mut keys: Vec<String> = CORE_OVERRIDES.iter().map(|f| f.to_string()).collect();
    let attr_keys = cur.fields.attributes.keys().chain(fields.attributes.keys());
    for k in attr_keys {
        let f = attribute_field(k);
        if !keys.contains(&f) {
            keys.push(f);
        }
    }
    let changed: Vec<String> = keys
        .into_iter()
        .filter(|f| cur.fields.value_of(f) != fields.value_of(f))
        .collect();
    if changed.is_empty() {
        return Ok(changed);
    }
    let mut overrides = cur.overrides.clone();
    for f in &changed {
        if !overrides.contains(f) {
            overrides.push(f.clone());
        }
    }
    let side = |v: &StudentFields, o: &[String]| {
        let mut m: Map<String, Json> = changed.iter().map(|f| (f.clone(), v.value_of(f))).collect();
        m.insert("manual_overrides".into(), json!(o));
        Json::Object(m)
    };
    let (before, after) = (side(&cur.fields, &cur.overrides), side(fields, &overrides));
    let same = Statement::new(
        "EXISTS (SELECT 1 FROM student WHERE id = ? AND display_name = ? AND email IS ? \
         AND locale = ? AND attributes = ? AND manual_overrides = ?)",
    )
    .bind(student_id)
    .bind(cur.fields.display_name.clone())
    .bind(cur.fields.email.clone())
    .bind(cur.fields.locale.0.clone())
    .bind(cur.raw_attributes.clone())
    .bind(cur.raw_overrides.clone());
    let (audit, gate) = audit::entry_if(
        s,
        actor,
        "student.update",
        Some(&target(student_id)),
        Some(before),
        Some(after),
        ctx,
        same,
    )?;
    let update = gated_stmt(
        "UPDATE student SET display_name = ?, email = ?, locale = ?, attributes = ?, \
         manual_overrides = ?, updated_at = ? WHERE id = ?",
        &gate,
        vec![
            crate::Value::from(fields.display_name.clone()),
            crate::Value::from(super::email::normalise(fields.email.as_deref())),
            crate::Value::from(fields.locale.0.clone()),
            crate::Value::from(Json::Object(fields.attributes.clone()).to_string()),
            crate::Value::from(json!(overrides).to_string()),
            crate::Value::from(s.now_ms()),
            crate::Value::from(student_id),
        ],
    );
    let done = s.db.batch(vec![audit, update]).await?;
    if affected(&done, 1) != 1 {
        return Err(changed_elsewhere());
    }
    Ok(changed)
}

/// "ใช้ค่าจากทะเบียน": takes `field` off the Student's `manual_overrides`,
/// so the next roster import sets it again (the value stays until then).
/// Audit `student.override_clear` with the list before and after.
///
/// `Err(Error::Invalid)` for an unknown Student, `Err(Error::Conflict)`
/// when `field` is not overridden or the list changed since it was read.
pub async fn clear_override(
    d: &Deployment,
    student_id: &str,
    field: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<()> {
    let s = d.services();
    let cur = load(s, student_id).await?;
    if !cur.overrides.iter().any(|o| o == field) {
        return Err(Error::Conflict(format!("{field} is not overridden")));
    }
    let rest: Vec<&String> = cur.overrides.iter().filter(|o| *o != field).collect();
    let same =
        Statement::new("EXISTS (SELECT 1 FROM student WHERE id = ? AND manual_overrides = ?)")
            .bind(student_id)
            .bind(cur.raw_overrides.clone());
    let (audit, gate) = audit::entry_if(
        s,
        actor,
        "student.override_clear",
        Some(&target(student_id)),
        Some(json!({ "manual_overrides": cur.overrides })),
        Some(json!({ "manual_overrides": rest })),
        ctx,
        same,
    )?;
    let update = gated_stmt(
        "UPDATE student SET manual_overrides = ?, updated_at = ? WHERE id = ?",
        &gate,
        vec![
            crate::Value::from(json!(rest).to_string()),
            crate::Value::from(s.now_ms()),
            crate::Value::from(student_id),
        ],
    );
    let done = s.db.batch(vec![audit, update]).await?;
    if affected(&done, 1) != 1 {
        return Err(changed_elsewhere());
    }
    Ok(())
}

/// What [`set_status`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct StatusChange {
    /// The status was different and is now the one asked for.
    pub changed: bool,
    /// Links revoked by this change (setting `left`).
    pub revoked: usize,
    /// Of those, how many accounts were told.
    pub notified: usize,
    /// Notices (or revokes of a racing Manual Link) that failed after the
    /// change was committed. The change stands; the Core has no logger, so
    /// this count is how the failure is reported (the page shows it).
    pub failures: usize,
}

/// Sets the Student's status by hand to `active` or `left` (audit
/// `student.status`) and adds `status` to `manual_overrides`, so a roster
/// import keeps it until "ใช้ค่าจากทะเบียน". Setting `left` revokes every live Link in the same
/// batch (each with its own `link.revoke` audit row, as
/// [`crate::links::revoke_by_staff`] does), then locks those accounts'
/// Menus and Pushes the revoked notice. Setting `active` re-creates no
/// Link. The same status again is a no-op (`changed == false`).
///
/// `Err(Error::Invalid)` for an unknown Student or status,
/// `Err(Error::Conflict)` when the status changed meanwhile.
pub async fn set_status(
    d: &Deployment,
    student_id: &str,
    status: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<StatusChange> {
    if !matches!(status, "active" | "left") {
        return Err(Error::Invalid(format!("status {status}")));
    }
    let s = d.services();
    let cur = load(s, student_id).await?;
    if cur.status == status {
        return Ok(StatusChange::default());
    }
    let mut overrides = cur.overrides.clone();
    if !overrides.iter().any(|o| o == "status") {
        overrides.push("status".into());
    }
    let same = Statement::new(
        "EXISTS (SELECT 1 FROM student WHERE id = ? AND status = ? AND manual_overrides = ?)",
    )
    .bind(student_id)
    .bind(cur.status.clone())
    .bind(cur.raw_overrides.clone());
    let (audit, gate) = audit::entry_if(
        s,
        actor,
        "student.status",
        Some(&target(student_id)),
        Some(json!({ "status": cur.status, "manual_overrides": cur.overrides })),
        Some(json!({ "status": status, "manual_overrides": overrides })),
        ctx,
        same,
    )?;
    let update = gated_stmt(
        "UPDATE student SET status = ?, manual_overrides = ?, updated_at = ? WHERE id = ?",
        &gate,
        vec![
            crate::Value::from(status),
            crate::Value::from(json!(overrides).to_string()),
            crate::Value::from(s.now_ms()),
            crate::Value::from(student_id),
        ],
    );
    let mut stmts = vec![audit, update];
    let default = &d.config().default_locale;
    let live = if status == "left" {
        links::live_links_of(s, student_id, default).await?
    } else {
        Vec::new()
    };
    for l in &live {
        stmts.extend(links::staff_revoke_statements(
            s,
            l,
            actor,
            ctx,
            Some(&gate),
        )?);
    }
    let done = s.db.batch(stmts).await?;
    if affected(&done, 1) != 1 {
        return Err(changed_elsewhere());
    }
    let mut out = StatusChange {
        changed: true,
        ..StatusChange::default()
    };
    // Committed: from here on a failure is counted, never returned.
    for (i, l) in live.into_iter().enumerate() {
        // Each Link adds three statements; its update is the second.
        if affected(&done, 2 + 3 * i + 1) == 1 {
            out.revoked += 1;
            match links::notify_revoked(d, l).await {
                Ok(true) => out.notified += 1,
                Ok(false) => {}
                Err(_) => out.failures += 1,
            }
        }
    }
    if status == "left" {
        // A Manual Link that raced the change: revoke it too.
        let raced = links::live_links_of(s, student_id, default).await;
        for l in raced.unwrap_or_default() {
            match links::revoke_by_staff(d, &l.id, actor, ctx).await {
                Ok(c) => {
                    out.revoked += 1;
                    out.notified += usize::from(c.notified);
                }
                Err(Error::Conflict(_)) => {}
                Err(_) => out.failures += 1,
            }
        }
    }
    Ok(out)
}

/// Logs that `actor` opened the Student's page: audit `student.view`, a
/// sensitive read (no before/after).
pub async fn record_view(
    d: &Deployment,
    student_id: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<()> {
    let target = target(student_id);
    audit::record(
        d.services(),
        actor,
        "student.view",
        Some(&target),
        None,
        None,
        ctx,
    )
    .await
}
