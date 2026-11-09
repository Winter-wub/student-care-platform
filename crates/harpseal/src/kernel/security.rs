//! Security Events: append-only records of suspicious acts around
//! Verification (spec, "Kernel data model"). Kinds written by the chat
//! flow: `link.duplicate_attempt`, `link.shared_email` (#71), `otp.lockout`; the webhook path writes
//! `inbound.failed`; Staff sign-in writes `staff.lockout`. `details` never
//! carries a secret (no code, no hash).

use crate::port_types::Statement;
use crate::{Result, Services};

/// The insert for one event, for use inside a `Db::batch`. `details` is a
/// flat JSON object of text values.
pub(crate) fn statement(
    services: &Services,
    kind: &str,
    channel_account_id: Option<&str>,
    student_id: Option<&str>,
    details: &[(&str, String)],
) -> Result<Statement> {
    insert(
        services,
        kind,
        channel_account_id,
        student_id,
        None,
        details,
    )
}

fn insert(
    services: &Services,
    kind: &str,
    channel_account_id: Option<&str>,
    student_id: Option<&str>,
    staff_id: Option<&str>,
    details: &[(&str, String)],
) -> Result<Statement> {
    let placeholders = vec!["?, ?"; details.len()].join(", ");
    let sql = format!(
        "INSERT INTO security_event (id, at, kind, channel_account_id, student_id, staff_id, \
         details) VALUES (?, ?, ?, ?, ?, ?, json_object({placeholders}))"
    );
    let mut stmt = Statement::new(sql)
        .bind(services.new_id()?)
        .bind(services.now_ms())
        .bind(kind)
        .bind(channel_account_id.map(str::to_string))
        .bind(student_id.map(str::to_string))
        .bind(staff_id.map(str::to_string));
    for (k, v) in details {
        stmt = stmt.bind(*k).bind(v.clone());
    }
    Ok(stmt)
}

/// Writes one Security Event now.
pub(crate) async fn record(
    services: &Services,
    kind: &str,
    channel_account_id: Option<&str>,
    student_id: Option<&str>,
    details: &[(&str, String)],
) -> Result<()> {
    let stmt = statement(services, kind, channel_account_id, student_id, details)?;
    services.db.execute(stmt).await.map(|_| ())
}

/// Writes one Security Event about a Staff member (e.g. `staff.lockout`).
pub(crate) async fn record_staff(
    services: &Services,
    kind: &str,
    staff_id: &str,
    details: &[(&str, String)],
) -> Result<()> {
    let stmt = insert(services, kind, None, None, Some(staff_id), details)?;
    services.db.execute(stmt).await.map(|_| ())
}

/// What Staff read for a Security Event kind: the text
/// `kernel.admin.kind.<kind with _>` when the catalog has it, else the
/// kind itself.
pub(crate) fn kind_label(catalog: &crate::Catalog, locale: &crate::Locale, kind: &str) -> String {
    let key = format!("kernel.admin.kind.{}", kind.replace('.', "_"));
    if catalog.knows(&key) {
        catalog.t(locale, &key)
    } else {
        kind.to_string()
    }
}
