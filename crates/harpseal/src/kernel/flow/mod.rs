//! The chat flow (spec, "Chat flow"; ticket #9): the Kernel answers every
//! Inbound of an account without a live Link itself, so Modules never see
//! one. State lives on `channel_account.flow_state`:
//!
//! ```text
//! new ─Follow/any─▶ awaiting_consent ─accept─▶ awaiting_student_id
//!     ─valid id─▶ awaiting_otp ─right code─▶ linked
//! linked ─notice version changed─▶ awaiting_reconsent ─accept─▶ linked
//! linked ─unlink─▶ awaiting_student_id (Consent kept)
//! ```
//!
//! Each Inbound gets one batch of answers (one reply token); warnings to
//! other accounts go as Pushes afterwards. Kernel Actions:
//! `kernel.consent.accept`, `kernel.link.start` (the locked Menu's
//! "เริ่มใช้งาน" entry: a Deployment passes it to channels with their own
//! locked Menu, e.g. `LineConfig::with_locked_menu`; it behaves like a
//! Follow), `kernel.otp.resend`, `kernel.otp.cancel`, `kernel.link.unlink`
//! (+ `_confirm` / `_keep`).

mod account;
mod claim;
mod consent;
mod linked;
mod otp;

use crate::delivery::{ChannelAccountId, Inbound, IncomingFile, ReplyToken};
use crate::deployment::Deployment;
use crate::kernel::text::Texts;
use crate::message::{ActionId, Button, Choices, Locale, Message};
use crate::{Result, Services};

pub(crate) use account::mark_unreachable;
use account::{Account, FlowData};

/// Kernel Action ids.
pub(crate) mod actions {
    /// Accept the Consent Card.
    pub(crate) const CONSENT_ACCEPT: &str = "kernel.consent.accept";
    /// The locked Menu's start entry; same as a Follow.
    pub(crate) const START: &str = "kernel.link.start";
    /// Send a new code.
    pub(crate) const OTP_RESEND: &str = "kernel.otp.resend";
    /// Give up the code and type another student id.
    pub(crate) const OTP_CANCEL: &str = "kernel.otp.cancel";
    /// The Menu's unlink entry.
    pub(crate) const UNLINK: &str = "kernel.link.unlink";
    /// Confirms the unlink.
    pub(crate) const UNLINK_CONFIRM: &str = "kernel.link.unlink_confirm";
    /// Keeps the Link.
    pub(crate) const UNLINK_KEEP: &str = "kernel.link.unlink_keep";
}

/// An Inbound, reduced to what the flow branches on.
#[derive(Debug, Clone)]
pub(super) enum Input {
    /// Follow, Unblocked, `/start`, or the locked Menu's start entry.
    Start,
    Text(String),
    File(IncomingFile),
    Action(ActionId),
}

/// One Inbound being answered.
pub(super) struct Turn<'a> {
    pub(super) d: &'a Deployment,
    pub(super) from: ChannelAccountId,
    reply: Option<ReplyToken>,
    pub(super) acct: Account,
    /// In the default Locale before Link, the Student's after.
    pub(super) texts: Texts,
    /// The current notice version.
    pub(super) version: String,
    /// The answer batch.
    pub(super) out: Vec<Message>,
    /// Recorded on the outbound rows: `kernel`, or the Module answering.
    pub(super) module: &'static str,
    /// Pushes to other accounts, sent after the answer.
    pub(super) pushes: Vec<(ChannelAccountId, Locale, Message)>,
}

/// Answers one Inbound.
pub(crate) async fn handle(d: &Deployment, ev: Inbound) -> Result<()> {
    let s = d.services();
    let from = ev.from().clone();
    let blocked = matches!(ev, Inbound::Blocked { .. });
    s.db.execute(account::ensure(s, &from, blocked)?).await?;
    if blocked {
        s.db.batch(mark_unreachable(s, &from)).await?;
        return Ok(());
    }
    let texts = Texts::load(s, d.catalog_arc(), d.config().default_locale.clone()).await?;
    let version = texts.consent_version(&d.config().consent.version);
    let acct = account::load(s, &from, &version).await?;
    let reply = ev.reply().cloned();
    let input = match ev {
        Inbound::Text { text, .. } => Input::Text(text),
        Inbound::File { file, .. } => Input::File(file),
        Inbound::Action { action, .. } if action.as_str() == actions::START => Input::Start,
        Inbound::Action { action, .. } => Input::Action(action),
        _ => Input::Start,
    };
    let mut turn = Turn {
        d,
        from,
        reply,
        acct,
        texts,
        version,
        out: Vec::new(),
        module: super::NAME,
        pushes: Vec::new(),
    };
    match turn.acct.link.clone() {
        Some(link) => linked::handle(&mut turn, link, input).await?,
        None => prelink(&mut turn, input).await?,
    }
    turn.flush().await
}

/// Every state without a live Link.
async fn prelink(t: &mut Turn<'_>, input: Input) -> Result<()> {
    let accept = |a: &ActionId| a.as_str() == actions::CONSENT_ACCEPT;
    match (t.acct.state.as_str(), input) {
        (_, Input::Start) => consent::offer(t, true).await,
        ("new" | "awaiting_consent", Input::Action(a)) if accept(&a) => {
            consent::accept_before_link(t).await
        }
        ("new", _) => consent::offer(t, true).await,
        ("awaiting_consent", _) => consent::offer(t, false).await,
        ("awaiting_otp", Input::Text(answer)) => otp::answer(t, &answer).await,
        ("awaiting_otp", Input::Action(a)) if a.as_str() == actions::OTP_RESEND => {
            otp::resend(t).await
        }
        ("awaiting_otp", Input::Action(a)) if a.as_str() == actions::OTP_CANCEL => {
            otp::cancel(t).await
        }
        ("awaiting_otp", _) => {
            otp::reprompt(t);
            Ok(())
        }
        ("awaiting_student_id", Input::Text(id)) => claim::student_id(t, &id).await,
        ("awaiting_student_id", _) => {
            t.say("kernel.link.ask_student_id");
            Ok(())
        }
        // `linked` / `awaiting_reconsent` with no live Link left (revoked
        // by Staff, or the Student left the roster): start over.
        _ => consent::offer(t, true).await,
    }
}

impl Turn<'_> {
    pub(super) fn s(&self) -> &Services {
        self.d.services()
    }
    pub(super) fn t(&self, key: &str) -> String {
        self.texts.t(key)
    }
    pub(super) fn tf(&self, key: &str, args: &[(&str, &str)]) -> String {
        self.texts.tf(key, args)
    }
    /// Adds the Text `key` to the answer.
    pub(super) fn say(&mut self, key: &str) {
        let text = self.t(key);
        self.out.push(Message::text(text));
    }
    pub(super) fn action(&self, key: &str, id: &str) -> Result<Button> {
        Ok(Button::action(self.t(key), ActionId::new(id)?))
    }

    /// Writes `state` and the account's `flow_data`.
    pub(super) fn save(&self, state: &str) -> crate::port_types::Statement {
        account::save(self.s(), &self.acct.id, state, &self.acct.flow)
    }

    /// Resets `flow_data` to what survives a finished or abandoned claim.
    pub(super) fn reset_flow(&mut self) {
        self.acct.flow = FlowData {
            consent_version: self.acct.flow.consent_version.take(),
            last_fallback_at: self.acct.flow.last_fallback_at,
            ..FlowData::default()
        };
    }

    /// Unlocks (`Some` Menu in `locale`) or locks (`None`) the Menu.
    pub(super) async fn set_menu(&self, unlocked: Option<&Locale>) -> Result<()> {
        let channel = self
            .d
            .channel(self.from.channel.as_str())
            .ok_or_else(|| crate::Error::Invalid(format!("no channel {}", self.from.channel)))?;
        let menu = unlocked.map(|l| self.d.menu_for(l));
        channel.set_menu(&self.from, menu.as_ref()).await
    }

    /// Adds Choices listing the Menu once (`kernel.menu.pick`), or the bare
    /// text when the Menu is empty.
    pub(super) fn menu_choices(&mut self) {
        let text = self.t("kernel.menu.pick");
        let options: Vec<Button> = self
            .d
            .menu_for(self.texts.locale())
            .entries
            .into_iter()
            .map(|e| Button::action(e.label, e.action))
            .collect();
        self.out.push(if options.is_empty() {
            Message::text(text)
        } else {
            Message::Choices(Choices { text, options })
        });
    }

    /// Queues a Push of the Text `key` to every account in `to` (kind,
    /// external id), in `locale`.
    pub(super) fn warn(&mut self, to: Vec<(String, String)>, key: &str, locale: &Locale) {
        let text = self.texts.in_locale(locale).t(key);
        for (kind, external_id) in to {
            let Some(channel) = self.d.channel(&kind) else {
                continue;
            };
            let account = ChannelAccountId::new(channel.kind(), external_id);
            self.pushes
                .push((account, locale.clone(), Message::text(text.clone())));
        }
    }

    /// Sends the answer as one batch, then the Pushes. A failed Push does
    /// not fail the Inbound (an unreachable account is marked as such by
    /// the send path).
    async fn flush(self) -> Result<()> {
        let module = Some(self.module);
        let locale = self.texts.locale().clone();
        self.d
            .deliver(&self.from, self.reply.as_ref(), &self.out, &locale, module)
            .await?;
        for (to, locale, msg) in &self.pushes {
            let _ = self
                .d
                .deliver(to, None, std::slice::from_ref(msg), locale, module)
                .await;
        }
        Ok(())
    }
}

/// Minutes from `now` until `until`, rounded up, at least 1.
pub(super) fn minutes_left(now: i64, until: i64) -> String {
    ((until - now + 59_999) / 60_000).max(1).to_string()
}
