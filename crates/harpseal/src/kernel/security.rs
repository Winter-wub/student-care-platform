//! Security Events: append-only records of suspicious acts around
//! Verification (spec, "Kernel data model"). Kinds written by the chat
//! flow: `link.duplicate_attempt`, `otp.lockout`; the webhook path writes
//! `inbound.failed`. `details` never carries a secret (no code, no hash).

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
    let placeholders = vec!["?, ?"; details.len()].join(", ");
    let sql = format!(
        "INSERT INTO security_event (id, at, kind, channel_account_id, student_id, details) \
         VALUES (?, ?, ?, ?, ?, json_object({placeholders}))"
    );
    let mut stmt = Statement::new(sql)
        .bind(services.new_id()?)
        .bind(services.now_ms())
        .bind(kind)
        .bind(channel_account_id.map(str::to_string))
        .bind(student_id.map(str::to_string));
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
