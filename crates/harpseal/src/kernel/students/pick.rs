//! The Manual Link picker (#143): the recent Channel Accounts with no live
//! Link, newest activity first, so Staff pick the account the Student
//! just wrote from instead of typing its raw id. The raw id stays behind a
//! disclosure for support cases.
//!
//! Only what is already stored is shown: the Channel, the account id
//! (shortened; the full id is the radio's value), when it first wrote,
//! when it last wrote and where it stopped in the chat flow. The Kernel
//! keeps no display name and no message text for an account that is not
//! linked (what such an account types is the Verification flow itself), so
//! neither is shown, and rendering calls no Channel API.
//!
//! The order follows only what the account itself did: its first contact
//! and its newest claimed webhook event. Staff actions (a revoke, a roster
//! import that marks Students left) also touch `channel_account`, and must
//! never lift an account above one that just wrote. The list is read only
//! when Staff open it ([`recent`], or `?pick=1` on the Student's page), not
//! on every view of the page.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};

use super::read::{self, int, text};
use super::view::{View, is_htmx};
use crate::deployment::inbound::INBOUND_EVENT_DAYS;
use crate::port_types::{Statement, Value};
use crate::{AppState, ChannelKind, CurrentStaff, Result, Services};

/// Accounts the picker lists at most.
pub(super) const RECENT_CAP: usize = 20;

/// How far back the picker looks: webhook events are kept this long.
const WINDOW_MS: i64 = INBOUND_EVENT_DAYS * 86_400_000;

/// The posted field of a picked account: `{channel}:{external_id}`.
pub(super) const PICK: &str = "pick";

/// One Channel Account with no live Link.
pub(super) struct Recent {
    pub(super) channel: String,
    pub(super) external_id: String,
    pub(super) first_seen: i64,
    /// The later of its first contact and its newest claimed webhook event.
    pub(super) last_seen: i64,
    pub(super) state: String,
    pub(super) blocked: bool,
}

/// What the picker shows: the newest [`RECENT_CAP`] accounts, and whether
/// there were more.
pub(super) struct Picker {
    pub(super) recent: Vec<Recent>,
    pub(super) more: bool,
    /// When it was read, for "2 นาทีที่แล้ว".
    pub(super) now: i64,
}

/// The picker's query for `channels` Channels. Binds, in order: the window
/// start twice (events, then new accounts), each Channel, the row limit.
/// The recent events and new accounts drive the join (`CROSS JOIN` keeps
/// that order), so every table is reached through an index (the query plan
/// is tested).
#[doc(hidden)]
pub fn recent_sql(channels: usize) -> String {
    let marks = vec!["?"; channels.max(1)].join(", ");
    format!(
        "SELECT ca.channel, ca.external_id, ca.first_seen_at, ca.flow_state, ca.blocked_at, \
         max(ca.first_seen_at, coalesce((SELECT max(ie.received_at) FROM inbound_event ie \
         WHERE ie.channel = ca.channel AND ie.external_id = ca.external_id), 0)) AS last_seen \
         FROM (SELECT DISTINCT id FROM (SELECT c.id FROM inbound_event e JOIN channel_account c \
         ON c.channel = e.channel AND c.external_id = e.external_id WHERE e.received_at >= ? \
         UNION ALL SELECT n.id FROM channel_account n WHERE n.first_seen_at >= ?)) cand \
         CROSS JOIN channel_account ca ON ca.id = cand.id \
         WHERE ca.channel IN ({marks}) AND NOT EXISTS (SELECT 1 FROM link l \
         WHERE l.channel_account_id = ca.id AND l.status <> 'revoked') \
         ORDER BY ca.blocked_at IS NOT NULL, last_seen DESC, ca.id LIMIT ?"
    )
}

/// Reads the picker for `channels` at the Services' clock.
pub(super) async fn load(s: &Services, channels: &[ChannelKind]) -> Result<Picker> {
    let now = s.now_ms();
    if channels.is_empty() {
        return Ok(Picker {
            recent: Vec::new(),
            more: false,
            now,
        });
    }
    let since = now - WINDOW_MS;
    let mut stmt = Statement::new(recent_sql(channels.len()))
        .bind(since)
        .bind(since);
    for c in channels {
        stmt = stmt.bind(c.as_str());
    }
    let rows = s.db.query(stmt.bind(RECENT_CAP as i64 + 1)).await?;
    let recent = (0..rows.len().min(RECENT_CAP))
        .map(|i| Recent {
            channel: text(&rows, i, "channel").unwrap_or_default(),
            external_id: text(&rows, i, "external_id").unwrap_or_default(),
            first_seen: int(&rows, i, "first_seen_at").unwrap_or(0),
            last_seen: int(&rows, i, "last_seen").unwrap_or(0),
            state: text(&rows, i, "flow_state").unwrap_or_default(),
            blocked: rows.get(i, "blocked_at").is_some_and(|v| *v != Value::Null),
        })
        .collect();
    Ok(Picker {
        recent,
        more: rows.len() > RECENT_CAP,
        now,
    })
}

/// `GET /admin/students/{id}/links/recent` (Manual Link Permission): the
/// picker's list as an HTMX fragment. Without HTMX it goes to the
/// Student's page with the list open (`?pick=1`), so it works without
/// script. Not found for an unknown Student or one who has left.
pub(super) async fn recent(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Response {
    let v = View::new(&s, &staff);
    let row = match read::student(&s.services, &id).await {
        Ok(Some(row)) if row.status == "active" => row,
        Ok(_) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    let base = format!("/admin/students/{}", row.id);
    if !is_htmx(&headers) {
        return Redirect::to(&format!("{base}?pick=1#ml-pick")).into_response();
    }
    match load(&s.services, &s.deployment.staff_channel_kinds()).await {
        Ok(p) => {
            let body = super::pick_view::list(&v, &base, &p);
            (StatusCode::OK, Html(body.into_string())).into_response()
        }
        Err(e) => v.error(&e),
    }
}

/// `id` as Staff read it in the list: a long platform id keeps its first
/// 6 and last 4 characters.
pub(super) fn short_id(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    if chars.len() <= 14 {
        return id.to_string();
    }
    let head: String = chars[..6].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

/// The account a Manual Link post names: the picked one, else the typed
/// Channel and id. `None` when both are given and differ (no guess).
pub(super) fn chosen<'a>(
    pick: &'a str,
    channel: &'a str,
    typed: &'a str,
) -> Option<(&'a str, &'a str)> {
    let picked = pick
        .split_once(':')
        .filter(|(c, id)| !c.is_empty() && !id.is_empty());
    match (picked, typed.trim()) {
        (Some(p), "") => Some(p),
        (Some((c, id)), t) if c == channel && id == t => Some((c, id)),
        (Some(_), _) => None,
        (None, _) => Some((channel, typed)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_ids_keep_their_ends() {
        assert_eq!(short_id("U4af4980629a1b2c3d4e5f60718293a4b"), "U4af49…3a4b");
        assert_eq!(short_id("7012345678"), "7012345678");
        assert_eq!(short_id("ก".repeat(20).as_str()), "กกกกกก…กกกก");
    }

    #[test]
    fn the_pick_wins_over_an_empty_typed_id_and_never_over_another() {
        assert_eq!(chosen("line:Uabc", "telegram", ""), Some(("line", "Uabc")));
        assert_eq!(
            chosen("line:Uabc", "telegram", "  "),
            Some(("line", "Uabc"))
        );
        assert_eq!(
            chosen("line:Uabc", "line", " Uabc "),
            Some(("line", "Uabc"))
        );
        assert_eq!(chosen("line:Uabc", "line", "Uxyz"), None);
        assert_eq!(chosen("", "telegram", "701"), Some(("telegram", "701")));
        assert_eq!(chosen("fake:a:b", "line", ""), Some(("fake", "a:b")));
        assert_eq!(chosen(":x", "line", "Uq"), Some(("line", "Uq")));
    }

    #[test]
    fn the_query_binds_one_mark_per_channel() {
        assert!(recent_sql(2).contains("ca.channel IN (?, ?)"));
        assert!(recent_sql(0).contains("ca.channel IN (?)"));
    }
}
