//! Menu variants (#182): which variant a linked Student or account has,
//! and the same choice in SQL for the Jobs that re-link accounts in bulk.

use serde_json::{Map, Value as Json};

use crate::config::MenuVariant;
use crate::port_types::{Rows, Statement, Value};
use crate::{Deployment, Result};

/// The Menu variant of Student `student`; `None` (without a query) when
/// the Deployment has no variants.
pub(crate) async fn of_student<'d>(
    d: &'d Deployment,
    student: &str,
) -> Result<Option<&'d MenuVariant>> {
    if d.config().menu.variants.is_empty() {
        return Ok(None);
    }
    let st = Statement::new("SELECT attributes FROM student WHERE id = ?").bind(student);
    Ok(first(d, &d.services().db.query(st).await?))
}

/// The Menu variant of the Student linked to Channel Account `account`
/// (`channel_account.id`), if any; `None` without a query when the
/// Deployment has no variants.
pub(crate) async fn of_account<'d>(
    d: &'d Deployment,
    account: &str,
) -> Result<Option<&'d MenuVariant>> {
    if d.config().menu.variants.is_empty() {
        return Ok(None);
    }
    let st = Statement::new(
        "SELECT s.attributes FROM link l JOIN student s ON s.id = l.student_id \
         WHERE l.channel_account_id = ? AND l.status IN ('active', 'unreachable') LIMIT 1",
    )
    .bind(account);
    Ok(first(d, &d.services().db.query(st).await?))
}

/// The variant of the attributes in the first row's `attributes`.
fn first<'d>(d: &'d Deployment, rows: &Rows) -> Option<&'d MenuVariant> {
    let attrs = rows.get(0, "attributes").and_then(Value::as_text)?;
    let attrs: Map<String, Json> = serde_json::from_str(attrs).unwrap_or_default();
    d.menu_variant(&attrs)
}

/// The Students of group `group` as an SQL condition on the JSON column
/// `col`, with its values: group 0 matches no variant (the whole Menu),
/// group `i` is variant `i - 1` (it matches, and no earlier one does).
/// An empty condition (`"1"`) when there are no variants.
pub(crate) fn group_sql(variants: &[MenuVariant], group: usize, col: &str) -> (String, Vec<Value>) {
    let mut parts = Vec::new();
    let mut binds = Vec::new();
    let earlier = if group == 0 {
        variants.len()
    } else {
        group - 1
    };
    for v in &variants[..earlier.min(variants.len())] {
        let (sql, b) = v.sql(col);
        parts.push(format!("NOT {sql}"));
        binds.extend(b.into_iter().map(Value::Text));
    }
    if let Some(v) = group.checked_sub(1).and_then(|i| variants.get(i)) {
        let (sql, b) = v.sql(col);
        parts.push(sql);
        binds.extend(b.into_iter().map(Value::Text));
    }
    if parts.is_empty() {
        return ("1".into(), binds);
    }
    (parts.join(" AND "), binds)
}
