//! The Manual Link picker (#143): the recent Channel Accounts with no live
//! Link, newest activity first, so Staff pick the account the Student
//! just wrote from instead of typing its raw id. The raw id stays behind a
//! disclosure for support cases.
//!
//! Only what is already stored is shown: the Channel, the account id
//! (shortened; the full id is the radio's value), when it first wrote,
//! when it was last active and where it stopped in the chat flow. The
//! Kernel keeps no display name and no message text for an account that
//! is not linked (what such an account types is the Verification flow
//! itself), so neither is shown, and rendering calls no Channel API.

use maud::{Markup, PreEscaped, html};

use super::read::{int, text};
use super::view::{View, channel_label};
use crate::port_types::{Statement, Value};
use crate::{ChannelKind, Result, Services};

/// Accounts the picker lists at most.
pub(super) const RECENT_CAP: usize = 20;

/// The posted field of a picked account: `{channel}:{external_id}`.
pub(super) const PICK: &str = "pick";

/// One Channel Account with no live Link.
pub(super) struct Recent {
    pub(super) channel: String,
    pub(super) external_id: String,
    pub(super) first_seen: i64,
    /// The later of `channel_account.updated_at` and its newest claimed
    /// webhook event (kept 7 days): a message that does not move the flow
    /// still counts.
    pub(super) last_seen: i64,
    pub(super) state: String,
    pub(super) blocked: bool,
}

/// The newest [`RECENT_CAP`] accounts on `channels` with no live Link, and
/// whether there are more.
pub(super) async fn recent_unlinked(
    s: &Services,
    channels: &[ChannelKind],
) -> Result<(Vec<Recent>, bool)> {
    if channels.is_empty() {
        return Ok((Vec::new(), false));
    }
    let marks = vec!["?"; channels.len()].join(", ");
    let mut stmt = Statement::new(format!(
        "SELECT ca.channel, ca.external_id, ca.first_seen_at, ca.flow_state, ca.blocked_at, \
         max(ca.updated_at, coalesce(ie.last, 0)) AS last_seen FROM channel_account ca \
         LEFT JOIN (SELECT channel, external_id, max(received_at) AS last FROM inbound_event \
         WHERE external_id IS NOT NULL GROUP BY channel, external_id) ie \
         ON ie.channel = ca.channel AND ie.external_id = ca.external_id \
         WHERE ca.channel IN ({marks}) AND NOT EXISTS (SELECT 1 FROM link l \
         WHERE l.channel_account_id = ca.id AND l.status <> 'revoked') \
         ORDER BY last_seen DESC, ca.id LIMIT ?"
    ));
    for c in channels {
        stmt = stmt.bind(c.as_str());
    }
    let rows = s.db.query(stmt.bind(RECENT_CAP as i64 + 1)).await?;
    let more = rows.len() > RECENT_CAP;
    let list = (0..rows.len().min(RECENT_CAP))
        .map(|i| Recent {
            channel: text(&rows, i, "channel").unwrap_or_default(),
            external_id: text(&rows, i, "external_id").unwrap_or_default(),
            first_seen: int(&rows, i, "first_seen_at").unwrap_or(0),
            last_seen: int(&rows, i, "last_seen").unwrap_or(0),
            state: text(&rows, i, "flow_state").unwrap_or_default(),
            blocked: rows.get(i, "blocked_at").is_some_and(|v| *v != Value::Null),
        })
        .collect();
    Ok((list, more))
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

/// The flow stage's catalog key; a stage without its own text reads as
/// not started.
fn stage_key(state: &str) -> &'static str {
    match state {
        "awaiting_consent" => "kernel.students.pick_stage_consent",
        "awaiting_student_id" => "kernel.students.pick_stage_student_id",
        "awaiting_otp" => "kernel.students.pick_stage_otp",
        _ => "kernel.students.pick_stage_new",
    }
}

/// The Manual Link form's body: the hint, the list of recent accounts as
/// radios (none chosen), and the raw id fields behind a closed disclosure.
pub(super) fn fields(
    v: &View<'_>,
    recent: &[Recent],
    more: bool,
    offered: &[ChannelKind],
) -> Markup {
    html! {
        fieldset class="checks pick" aria-describedby="ml-pick-hint" {
            legend { (v.t("kernel.students.pick_title")) }
            p id="ml-pick-hint" class="muted" { (v.t("kernel.students.pick_hint")) }
            @if recent.is_empty() {
                p class="muted" { (v.t("kernel.students.pick_none")) }
            } @else {
                ul class="list" {
                    @for (i, r) in recent.iter().enumerate() {
                        li {
                            label class="choice" for=(format!("ml-pick-{i}")) {
                                input id=(format!("ml-pick-{i}")) type="radio" name=(PICK)
                                    value=(format!("{}:{}", r.channel, r.external_id));
                                span {
                                    span class="name" { (channel_label(&r.channel)) " " }
                                    span class="code" { (short_id(&r.external_id)) }
                                    br;
                                    span class="muted" {
                                        (v.t("kernel.students.pick_last")) " " (v.time(r.last_seen))
                                        " · " (v.t(stage_key(&r.state)))
                                        @if r.blocked { " · " (v.t("kernel.students.pick_blocked")) }
                                    }
                                    br;
                                    span class="muted" { (v.t("kernel.students.pick_first")) " " (v.time(r.first_seen)) }
                                }
                            }
                        }
                    }
                }
                @if more { p class="muted" { (v.t("kernel.students.pick_capped")) } }
            }
        }
        details class="pick-typed" {
            summary { (v.t("kernel.students.pick_typed")) }
            div class="row2" {
                div class="field" {
                    label for="ml-channel" { (v.t("kernel.students.channel")) }
                    select id="ml-channel" name="channel" {
                        @for kind in offered { option value=(kind.as_str()) { (channel_label(kind.as_str())) } }
                    }
                }
                div class="field" {
                    label for="ml-external" { (v.t("kernel.students.external_id")) }
                    input id="ml-external" name="external_id" type="text" autocomplete="off";
                }
            }
        }
        script { (PreEscaped(PICK_JS)) }
    }
}

/// Picking an account clears the typed id, and typing one clears the
/// pick, so the form never posts two different accounts; without script
/// the server refuses that instead of guessing. No library, no build step.
const PICK_JS: &str = r#"(function(){
var t=document.getElementById('ml-external');if(!t)return;var f=t.form;
f.addEventListener('change',function(e){if(e.target.name==='pick'&&e.target.checked)t.value='';});
t.addEventListener('input',function(){if(t.value)f.querySelectorAll('input[name=pick]').forEach(function(r){r.checked=false;});});
})();"#;

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
}
