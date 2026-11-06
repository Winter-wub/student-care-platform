//! The `channel_account` row of the account an Inbound came from: its
//! flow state, its `flow_data` (the in-flight claim) and its live Link.
//!
//! `flow_data` keys: `consent_version` (notice version the Student
//! accepted on the Card, written to `consent` on Link), `student_id` and
//! `challenge_id` (the open claim), `parked` (an Action id waiting for a
//! re-consent), `last_fallback_at` (ms of the last fallback answer).

use crate::delivery::ChannelAccountId;
use crate::message::Locale;
use crate::port_types::{Rows, Statement, Value};
use crate::{Error, Result, Services};

/// `flow_data`, read and rewritten whole.
#[derive(Debug, Clone, Default)]
pub(crate) struct FlowData {
    pub(crate) consent_version: Option<String>,
    pub(crate) student_id: Option<String>,
    pub(crate) challenge_id: Option<String>,
    pub(crate) parked: Option<String>,
    pub(crate) last_fallback_at: Option<i64>,
}

/// The account's live (active or unreachable) Link.
#[derive(Debug, Clone)]
pub(crate) struct LinkRow {
    pub(crate) id: String,
    pub(crate) unreachable: bool,
    pub(crate) student: String,
    pub(crate) locale: Locale,
    /// A `consent` row exists for the current notice version.
    pub(crate) current_consent: bool,
    /// Any `consent` row exists for the Student.
    pub(crate) any_consent: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Account {
    /// `channel_account.id`.
    pub(crate) id: String,
    pub(crate) state: String,
    pub(crate) flow: FlowData,
    pub(crate) link: Option<LinkRow>,
}

/// Creates the row on first contact; records or clears `blocked_at`.
pub(crate) fn ensure(
    services: &Services,
    from: &ChannelAccountId,
    blocked: bool,
) -> Result<Statement> {
    let now = services.now_ms();
    Ok(Statement::new(
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, blocked_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (channel, external_id) DO UPDATE SET \
         blocked_at = excluded.blocked_at",
    )
    .bind(services.new_id()?)
    .bind(from.channel.as_str())
    .bind(from.external_id.clone())
    .bind(now)
    .bind(blocked.then_some(now))
    .bind(now))
}

const LOAD: &str = "SELECT ca.id, ca.flow_state, \
    json_extract(ca.flow_data, '$.consent_version') AS cv, \
    json_extract(ca.flow_data, '$.student_id') AS fs, \
    json_extract(ca.flow_data, '$.challenge_id') AS ch, \
    json_extract(ca.flow_data, '$.parked') AS pk, \
    json_extract(ca.flow_data, '$.last_fallback_at') AS lf, \
    l.id AS link_id, l.status, l.student_id, s.locale, \
    EXISTS (SELECT 1 FROM consent c WHERE c.student_id = l.student_id AND c.notice_version = ?) AS cur, \
    EXISTS (SELECT 1 FROM consent c WHERE c.student_id = l.student_id) AS any_c \
    FROM channel_account ca \
    LEFT JOIN link l ON l.channel_account_id = ca.id AND l.status <> 'revoked' \
    LEFT JOIN student s ON s.id = l.student_id \
    WHERE ca.channel = ? AND ca.external_id = ?";

fn text(rows: &Rows, col: &str) -> Option<String> {
    rows.get(0, col)
        .and_then(Value::as_text)
        .map(str::to_string)
}

fn flag(rows: &Rows, col: &str) -> bool {
    rows.get(0, col).and_then(Value::as_i64) == Some(1)
}

/// Reads the account; `version` is the current notice version.
pub(crate) async fn load(
    services: &Services,
    from: &ChannelAccountId,
    version: &str,
) -> Result<Account> {
    let stmt = Statement::new(LOAD)
        .bind(version)
        .bind(from.channel.as_str())
        .bind(from.external_id.clone());
    let rows = services.db.query(stmt).await?;
    let id = text(&rows, "id").ok_or_else(|| Error::Db("channel_account row missing".into()))?;
    let link = match (text(&rows, "link_id"), text(&rows, "student_id")) {
        (Some(link_id), Some(student)) => Some(LinkRow {
            id: link_id,
            unreachable: text(&rows, "status").as_deref() == Some("unreachable"),
            student,
            locale: Locale(text(&rows, "locale").unwrap_or_else(|| "th".into())),
            current_consent: flag(&rows, "cur"),
            any_consent: flag(&rows, "any_c"),
        }),
        _ => None,
    };
    Ok(Account {
        id,
        state: text(&rows, "flow_state").unwrap_or_else(|| "new".into()),
        flow: FlowData {
            consent_version: text(&rows, "cv"),
            student_id: text(&rows, "fs"),
            challenge_id: text(&rows, "ch"),
            parked: text(&rows, "pk"),
            last_fallback_at: rows.get(0, "lf").and_then(Value::as_i64),
        },
        link,
    })
}

/// Writes `state` and the whole `flow_data`.
pub(crate) fn save(services: &Services, account: &str, state: &str, flow: &FlowData) -> Statement {
    Statement::new(
        "UPDATE channel_account SET flow_state = ?, flow_data = json_object(\
         'consent_version', ?, 'student_id', ?, 'challenge_id', ?, 'parked', ?, \
         'last_fallback_at', ?), updated_at = ? WHERE id = ?",
    )
    .bind(state)
    .bind(flow.consent_version.clone())
    .bind(flow.student_id.clone())
    .bind(flow.challenge_id.clone())
    .bind(flow.parked.clone())
    .bind(flow.last_fallback_at)
    .bind(services.now_ms())
    .bind(account)
}

/// The account's active Link becomes unreachable (LINE unfollow, or a send
/// refused with `Error::Unreachable`); `blocked_at` is set.
pub(crate) fn mark_unreachable(services: &Services, from: &ChannelAccountId) -> Vec<Statement> {
    let now = services.now_ms();
    vec![
        Statement::new(
            "UPDATE link SET status = 'unreachable', updated_at = ? WHERE status = 'active' \
             AND channel_account_id = (SELECT id FROM channel_account WHERE channel = ? AND external_id = ?)",
        )
        .bind(now)
        .bind(from.channel.as_str())
        .bind(from.external_id.clone()),
        Statement::new(
            "UPDATE channel_account SET blocked_at = ?, updated_at = ? WHERE channel = ? AND external_id = ?",
        )
        .bind(now)
        .bind(now)
        .bind(from.channel.as_str())
        .bind(from.external_id.clone()),
    ]
}

/// Cancels the account's open challenges.
pub(crate) fn cancel_challenges(account: &str) -> Statement {
    Statement::new(
        "UPDATE verification_challenge SET state = 'cancelled' \
         WHERE channel_account_id = ? AND state = 'pending'",
    )
    .bind(account)
}

/// An `audit_log` row for a Student's own act (actor = the Student).
pub(crate) fn audit(
    services: &Services,
    action: &str,
    student: &str,
    link: &str,
    after: &[(&str, String)],
) -> Result<Statement> {
    let placeholders = vec!["?, ?"; after.len()].join(", ");
    let sql = format!(
        "INSERT INTO audit_log (id, at, actor_kind, actor_id, action, target_kind, target_id, after) \
         VALUES (?, ?, 'student', ?, ?, 'link', ?, json_object({placeholders}))"
    );
    let mut stmt = Statement::new(sql)
        .bind(services.new_id()?)
        .bind(services.now_ms())
        .bind(student)
        .bind(action)
        .bind(link);
    for (k, v) in after {
        stmt = stmt.bind(*k).bind(v.clone());
    }
    Ok(stmt)
}

/// Accounts of the Student's other active Links: (channel kind, external
/// id), for warning Pushes. Excludes `except` (a `channel_account.id`).
pub(crate) async fn other_links(
    services: &Services,
    student: &str,
    except: &str,
    channel: Option<&str>,
) -> Result<Vec<(String, String)>> {
    let stmt = Statement::new(
        "SELECT ca.channel, ca.external_id FROM link l \
         JOIN channel_account ca ON ca.id = l.channel_account_id \
         WHERE l.student_id = ? AND l.status = 'active' AND ca.id <> ? \
         AND (? IS NULL OR l.channel = ?)",
    )
    .bind(student)
    .bind(except)
    .bind(channel.map(str::to_string))
    .bind(channel.map(str::to_string));
    let rows = services.db.query(stmt).await?;
    Ok(rows
        .rows
        .iter()
        .filter_map(|r| {
            Some((
                r.first()?.as_text()?.to_string(),
                r.get(1)?.as_text()?.to_string(),
            ))
        })
        .collect())
}
