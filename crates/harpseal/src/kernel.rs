//! Kernel pieces both designs share. Not public API (schema is internal).

use crate::config::DeploymentConfig;
use crate::delivery::{ChannelAccountId, OutboundMessage, StudentId};
use crate::error::BuildError;
use crate::manifest::Permission;
use crate::message::Locale;
use crate::port_types::{Rows, Statement, Value};

pub(crate) fn find_link(account: &ChannelAccountId) -> Statement {
    Statement::new(
        "SELECT student_id, locale FROM link \
         WHERE channel = ? AND external_id = ? AND status = 'active'",
    )
    .bind(account.channel.0)
    .bind(account.external_id.clone())
}

pub(crate) fn parse_link(rows: &Rows) -> Option<(StudentId, Locale)> {
    match rows.rows.first()?.as_slice() {
        [Value::Text(s), Value::Text(l)] => Some((StudentId(s.clone()), Locale(l.clone()))),
        _ => None,
    }
}

pub(crate) fn record_outbound(m: &OutboundMessage) -> Statement {
    Statement::new(
        "INSERT INTO outbound_message (channel, external_id, push, metered, message_id, sent_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(m.to.channel.0)
    .bind(m.to.external_id.clone())
    .bind(i64::from(m.push))
    .bind(i64::from(m.metering == crate::delivery::Metering::Metered))
    .bind(
        m.message_id
            .as_ref()
            .map(|i| i.0.clone())
            .unwrap_or_default(),
    )
    .bind(m.sent_at.0)
}

/// Every Role's Permission must exist; Permission ids must be unique.
pub(crate) fn check_permissions(
    defined: &[Permission],
    config: &DeploymentConfig,
) -> Result<(), BuildError> {
    for (i, p) in defined.iter().enumerate() {
        if defined[..i].iter().any(|q| q.id == p.id) {
            return Err(BuildError::DuplicatePermission(p.id));
        }
    }
    for (role, perms) in &config.roles {
        for perm in perms {
            if !defined.iter().any(|p| p.id == perm) {
                return Err(BuildError::UnknownPermission {
                    role: role.clone(),
                    permission: perm.clone(),
                });
            }
        }
    }
    Ok(())
}
