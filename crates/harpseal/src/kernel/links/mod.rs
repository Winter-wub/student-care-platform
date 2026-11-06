//! The Link service: every change to a Link after Verification goes
//! through here, so the audit row and the one-live-Link invariant live in
//! one place. The back office (Manual Link / revoke on a Student's page)
//! and the CLI call [`manual_link`] and [`revoke_by_staff`]; the chat
//! flow's self-unlink uses the same revoke statements with the Student as
//! the actor.
//!
//! Every write is one `Db::batch`. A revoke is gated on its own audit row
//! ([`crate::audit::entry_if`]): the row is inserted only while the Link
//! is live, and the rest of the batch runs only if that row exists, so of
//! two racing revokes exactly one changes anything and writes one audit
//! row. A Manual Link relies on the partial unique indexes on `link` (one
//! live Link per Student per Channel, and per Channel Account): a lost
//! race fails the whole batch and comes back as [`Error::Conflict`].
//!
//! The Pushes go out after the write. A failed Push does not undo it:
//! [`LinkChange::notified`] tells the caller whether the Student was told.

mod manual;
mod revoke;

pub use manual::manual_link;
pub use revoke::revoke_by_staff;
pub(crate) use revoke::{
    live_links_of, notify_revoked, revoke_statements, staff_revoke_statements,
};

use crate::audit::Actor;
use crate::delivery::ChannelAccountId;
use crate::deployment::Deployment;
use crate::message::{Locale, Menu, Message};
use crate::port_types::{Rows, Value};
use crate::{Error, Result};

/// What a Link change did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LinkChange {
    /// `link.id` of the Link created or revoked.
    pub link_id: String,
    /// The notice (and, for a Manual Link, the Consent Card) reached the
    /// Student's account. `false` when the account is blocked or the
    /// Channel refused the send; the change itself stands.
    pub notified: bool,
}

/// `revoked_by` / `verified_by_staff_id`: the Staff id, else `NULL`.
fn staff_id(actor: &Actor) -> Option<String> {
    match actor {
        Actor::Staff(id) => Some(id.clone()),
        _ => None,
    }
}

/// Column `col` of the first row, as text.
fn text(rows: &Rows, col: &str) -> Option<String> {
    rows.get(0, col)
        .and_then(Value::as_text)
        .map(str::to_string)
}

/// Locks (`None`) or unlocks the Menu, then Pushes `msgs`. `true` when
/// both went through.
async fn notify(
    d: &Deployment,
    to: &ChannelAccountId,
    menu: Option<Menu>,
    msgs: Vec<Message>,
    locale: &Locale,
) -> bool {
    let Ok(channel) = d.channel_or_err(to.channel.as_str()) else {
        return false;
    };
    let menu_set = channel.set_menu(to, menu.as_ref()).await.is_ok();
    let sent = d
        .deliver(to, None, &msgs, locale, Some(super::NAME))
        .await
        .is_ok();
    menu_set && sent
}

fn conflict(what: &str) -> Error {
    Error::Conflict(what.into())
}

/// The crate `Result`, shorter.
type R<T> = Result<T>;
