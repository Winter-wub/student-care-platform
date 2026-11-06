//! The Consent Card: before Verification (step 1–2) and again when the
//! notice version changes for a linked Student (re-consent, with the
//! triggering Action parked and replayed after accept).

use super::account::{self, LinkRow};
use super::{Input, Turn, actions, linked};
use crate::Result;
use crate::message::{ActionId, Button, Card, Message};
use crate::port_types::Statement;

/// The Card: summary, accept, and "read in full" as a Mini App button to
/// `/notice` when the Channel has a Mini App, else a line in the body.
fn card(t: &Turn<'_>, title_key: &str) -> Result<Message> {
    let mut body = t.t("kernel.consent.notice");
    let mut buttons = vec![t.action("kernel.consent.accept", actions::CONSENT_ACCEPT)?];
    let mini_app =
        t.d.channel(t.from.channel.as_str())
            .is_some_and(|c| c.mini_app().is_some());
    if mini_app {
        buttons.push(Button::mini_app(
            t.t("kernel.consent.read_full"),
            crate::kernel::notice::PATH,
        ));
    } else {
        body = format!("{body}\n\n{}", t.t("kernel.consent.read_full_hint"));
    }
    Ok(Message::Card(Card {
        title: t.t(title_key),
        body,
        buttons,
    }))
}

/// Step 1: (welcome Text +) Consent Card; any open claim is dropped. With
/// the welcome, the Menu is set to the locked one (LINE: the locked rich
/// menu from its config; Telegram: `/start` only).
pub(super) async fn offer(t: &mut Turn<'_>, welcome: bool) -> Result<()> {
    if welcome {
        t.say("kernel.flow.welcome");
        t.set_menu(None).await?;
    }
    let card = card(t, "kernel.consent.title")?;
    t.out.push(card);
    t.acct.flow = Default::default();
    let cancel = vec![account::cancel_challenges(&t.acct.id)];
    t.commit(cancel, "awaiting_consent").await?;
    Ok(())
}

/// Step 2: remember the version shown, ask for the student id.
pub(super) async fn accept_before_link(t: &mut Turn<'_>) -> Result<()> {
    t.acct.flow.consent_version = Some(t.version.clone());
    t.say("kernel.link.ask_student_id");
    t.save("awaiting_student_id").await?;
    Ok(())
}

/// The account waits on the Card but the Student already accepted the
/// current version (on another Channel): back to `linked`, replaying the
/// parked Action. `false` when a concurrent Inbound moved the state first.
pub(super) async fn already_accepted(t: &mut Turn<'_>, link: &LinkRow) -> Result<bool> {
    let parked = t.acct.flow.parked.take();
    if !t.save("linked").await? {
        return Ok(false);
    }
    if let Some(action) = parked.and_then(|p| ActionId::new(p).ok()) {
        linked::act(t, link, action).await?;
    }
    Ok(true)
}

/// A linked Student whose Consent is not current. Accept records it and
/// replays the parked Action (or lists the Menu); anything else re-sends
/// the Card, parking a Module Action and storing text or files.
pub(super) async fn reconsent(t: &mut Turn<'_>, link: LinkRow, input: Input) -> Result<()> {
    match input {
        Input::Action(a) if a.as_str() == actions::CONSENT_ACCEPT => {
            let parked = t.acct.flow.parked.take();
            let now = t.s().now_ms();
            let insert = Statement::new(
                "INSERT INTO consent (id, student_id, channel_account_id, notice_version, \
                 locale, accepted_at) VALUES (?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (student_id, notice_version) DO NOTHING",
            )
            .bind(t.s().new_id()?)
            .bind(link.student.clone())
            .bind(t.acct.id.clone())
            .bind(t.version.clone())
            .bind(link.locale.as_str())
            .bind(now);
            if !t.commit(vec![insert], "linked").await? {
                return Ok(());
            }
            t.set_menu(Some(&link.locale)).await?;
            match parked.and_then(|p| ActionId::new(p).ok()) {
                Some(action) => linked::act(t, &link, action).await,
                None => {
                    t.menu_choices();
                    Ok(())
                }
            }
        }
        other => {
            match other {
                Input::Action(a) if !a.as_str().starts_with("kernel.") => {
                    t.acct.flow.parked = Some(a.as_str().to_string());
                }
                Input::Text(_) | Input::File(_) => linked::store(t, &link, &other).await?,
                _ => {}
            }
            let card = card(t, "kernel.consent.updated")?;
            t.out.push(card);
            t.save("awaiting_reconsent").await?;
            Ok(())
        }
    }
}
