//! Identity, inbound events, Reply/Push, metering and Channel Capabilities.

pub use crate::inbound::{Inbound, IncomingFile, WebhookRequest};

/// Milliseconds since the Unix epoch, UTC, from the Deployment's Clock
/// (ADR-0007). Never measure durations with it on Workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub i64);

impl Timestamp {
    /// The value in milliseconds.
    pub fn ms(self) -> i64 {
        self.0
    }
    /// This instant plus `ms` milliseconds.
    pub fn plus_ms(self, ms: i64) -> Self {
        Timestamp(self.0.saturating_add(ms))
    }
}

/// Which Channel, e.g. `ChannelKind("line")`. Unique per Deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelKind(pub &'static str);

impl ChannelKind {
    /// The kind as text, as stored in `channel_account.channel`.
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl core::fmt::Display for ChannelKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0)
    }
}

/// A Student-side account on one Channel.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChannelAccountId {
    /// The Channel.
    pub channel: ChannelKind,
    /// The platform's user id (LINE user id, Telegram chat id).
    pub external_id: String,
}

impl ChannelAccountId {
    /// An account on `channel`.
    pub fn new(channel: ChannelKind, external_id: impl Into<String>) -> Self {
        Self {
            channel,
            external_id: external_id.into(),
        }
    }
}

/// Primary key of a Student row (a UUIDv7, ADR-0007), not the student code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StudentId(pub String);

/// Proof that we may Reply. LINE: a reply token valid for a short window.
/// Channels without the concept never issue one (everything is a Push).
#[derive(Debug, Clone)]
pub struct ReplyToken {
    /// The platform's token.
    pub token: String,
    /// After this instant the token is refused.
    pub expires_at: Timestamp,
}

/// How a message goes out. Chosen by the Core, never by a Module.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Delivery {
    /// In answer to what the Student just sent.
    Reply(ReplyToken),
    /// Started by the system.
    Push,
}

impl Delivery {
    /// Reply when an unexpired token exists, else Push. The only place the
    /// Reply/Push choice is made.
    pub fn select(token: Option<&ReplyToken>, now: Timestamp) -> Delivery {
        match token {
            Some(t) if t.expires_at > now => Delivery::Reply(t.clone()),
            _ => Delivery::Push,
        }
    }

    /// The reply token, when this is a Reply.
    pub fn reply_token(&self) -> Option<&ReplyToken> {
        match self {
            Delivery::Reply(t) => Some(t),
            Delivery::Push => None,
        }
    }
}

/// Whether one Outbound Message counted against a Quota on its Channel.
/// Reported by the adapter: Reply/Push alone does not decide it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Metering {
    /// Counted against the Channel's Quota.
    Metered,
    /// Not counted.
    Free,
}

/// The platform's id of a sent message (needed to edit or delete it).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlatformMessageId(pub String);

/// What a Channel returns for each Message it sent.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SendReceipt {
    /// The platform's id, when it gives one (the last part's id when the
    /// adapter split the Message into several platform messages).
    pub message_id: Option<PlatformMessageId>,
    /// `Metered` exactly when [`SendReceipt::metered_units`] is above 0:
    /// the flag stored on the `outbound_message` row.
    pub metering: Metering,
    /// How many metered units this Message cost. The metered unit is one
    /// Push request to one recipient (LINE counts requests, not message
    /// objects): a Message is charged one unit for each Push request it
    /// opens, so a Message spread over two Push requests costs 2, the other
    /// Messages sharing those requests cost 0, and a Reply costs 0. Summed
    /// over a batch it is the number of metered Push requests;
    /// `usage_daily.metered` adds these units.
    pub metered_units: u32,
    /// True when any part of this Message did not use the reply token: it
    /// went out by Push, whether the Core chose Push or the token was spent
    /// (used up by an earlier part, or past the Channel's reply limit).
    pub pushed: bool,
    /// How many of [`SendReceipt::metered_units`] belong to Push requests
    /// the platform answered as already accepted (LINE: 409 for a retry
    /// key it took on an earlier attempt). Those requests went out, and
    /// were paid for, then; nothing went out now. A resume from an
    /// adapter's token counts the units the stopped Message opened before
    /// the failure here too (in `metered_units` and `replayed_units`), so a
    /// receipt covers every request the Message opened, as on a resend of
    /// the whole batch. The Kernel meters a replayed unit only when no row
    /// recorded it (#44, #62).
    pub replayed_units: u32,
}

impl SendReceipt {
    /// A receipt for one Message: one metered unit when `metering` is
    /// `Metered`, none when `Free` (see [`SendReceipt::with_metered_units`]).
    pub fn new(message_id: Option<PlatformMessageId>, metering: Metering, pushed: bool) -> Self {
        Self {
            message_id,
            metering,
            metered_units: u32::from(metering == Metering::Metered),
            pushed,
            replayed_units: 0,
        }
    }

    /// The same receipt costing `units` metered units (`metering` follows:
    /// `Metered` when `units > 0`).
    pub fn with_metered_units(mut self, units: u32) -> Self {
        self.set_metered_units(units);
        self
    }

    /// Sets [`SendReceipt::metered_units`] and keeps `metering` in step.
    pub fn set_metered_units(&mut self, units: u32) {
        self.metered_units = units;
        self.metering = if units > 0 {
            Metering::Metered
        } else {
            Metering::Free
        };
    }

    /// Adds one metered unit (the Message opened another Push request).
    pub fn add_metered_unit(&mut self) {
        self.set_metered_units(self.metered_units.saturating_add(1));
    }

    /// The same receipt with `units` of its metered units replayed,
    /// clamped to [`SendReceipt::metered_units`].
    pub fn with_replayed_units(mut self, units: u32) -> Self {
        self.replayed_units = units.min(self.metered_units);
        self
    }

    /// Marks one more metered unit as replayed (its Push request was
    /// answered as already accepted), never beyond `metered_units`.
    pub fn add_replayed_unit(&mut self) {
        self.replayed_units = self
            .replayed_units
            .saturating_add(1)
            .min(self.metered_units);
    }
}

/// What [`crate::Channel::send_batch`] returns. `receipts[i]` belongs to
/// the batch's `i`-th Message. When `error` is set the batch stopped
/// early: the Messages after the last receipt were not sent, except that
/// `unfinished` (when set) reports the parts of Message `receipts.len()`
/// that went out before the failure (a Card List split into several
/// platform messages, a long text, keyboard follow-ups). An adapter
/// returns what went out instead of a bare error, so the Core can record
/// it and a retry can resume at Message `receipts.len()`.
#[derive(Debug)]
#[non_exhaustive]
pub struct BatchReceipt {
    /// One receipt per Message wholly sent, in order.
    pub receipts: Vec<SendReceipt>,
    /// The parts of the next Message that went out before `error` (their
    /// metered units and last id), if any did.
    pub unfinished: Option<SendReceipt>,
    /// Why the batch stopped early, if it did.
    pub error: Option<crate::Error>,
    /// Where the adapter can resume the batch when `error` is set
    /// ([`crate::Channel::send_batch_resumed`]); `None`: from the first
    /// part of Message `receipts.len()`.
    pub resume: Option<crate::ResumeToken>,
}

impl BatchReceipt {
    /// Every Message sent.
    pub fn complete(receipts: Vec<SendReceipt>) -> Self {
        Self {
            receipts,
            unfinished: None,
            error: None,
            resume: None,
        }
    }
    /// The batch stopped at `error` after sending `receipts.len()` whole
    /// Messages and none of the next one.
    pub fn partial(receipts: Vec<SendReceipt>, error: crate::Error) -> Self {
        Self {
            receipts,
            unfinished: None,
            error: Some(error),
            resume: None,
        }
    }
    /// The batch stopped at `error` after sending `receipts.len()` whole
    /// Messages and some parts of the next one (`unfinished`).
    pub fn part_way(
        receipts: Vec<SendReceipt>,
        unfinished: SendReceipt,
        error: crate::Error,
    ) -> Self {
        Self {
            receipts,
            unfinished: Some(unfinished),
            error: Some(error),
            resume: None,
        }
    }
    /// The same receipt with the adapter's resume token.
    pub fn with_resume(mut self, resume: Option<crate::ResumeToken>) -> Self {
        self.resume = resume;
        self
    }
    /// Whether anything at all went out.
    pub fn anything_sent(&self) -> bool {
        !self.receipts.is_empty() || self.unfinished.is_some()
    }
    /// Metered units of everything that went out, `unfinished` included.
    pub fn metered_units(&self) -> u32 {
        self.receipts
            .iter()
            .chain(&self.unfinished)
            .map(|r| r.metered_units)
            .sum()
    }
    /// The receipts, or the error when the batch stopped early (what went
    /// out is dropped: the Core's send path records it first).
    pub fn into_result(self) -> crate::Result<Vec<SendReceipt>> {
        match self.error {
            Some(e) => Err(e),
            None => Ok(self.receipts),
        }
    }
}

/// Optional things a Channel can do beyond the minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Capability {
    /// Edit a sent message.
    EditMessage,
    /// Delete a sent message.
    DeleteMessage,
    /// Open a Mini App.
    MiniApp,
    /// Send a file to a Student.
    SendAttachment,
    /// Send one request to many accounts.
    Multicast,
}

/// The set an adapter declares. Derived from its accessors, never configured.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Capabilities {
    /// [`Capability::EditMessage`].
    pub edit_message: bool,
    /// [`Capability::DeleteMessage`].
    pub delete_message: bool,
    /// [`Capability::MiniApp`].
    pub mini_app: bool,
    /// [`Capability::SendAttachment`].
    pub send_attachment: bool,
    /// [`Capability::Multicast`].
    pub multicast: bool,
}

impl Capabilities {
    /// Whether `c` is present.
    pub fn has(&self, c: Capability) -> bool {
        match c {
            Capability::EditMessage => self.edit_message,
            Capability::DeleteMessage => self.delete_message,
            Capability::MiniApp => self.mini_app,
            Capability::SendAttachment => self.send_attachment,
            Capability::Multicast => self.multicast,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metering_follows_the_units() {
        let mut r = SendReceipt::new(None, Metering::Free, true);
        assert_eq!((r.metered_units, r.metering), (0, Metering::Free));
        r.add_metered_unit();
        r.add_metered_unit();
        assert_eq!((r.metered_units, r.metering), (2, Metering::Metered));
        let r = r.with_metered_units(0);
        assert_eq!(r.metering, Metering::Free);
        let one = SendReceipt::new(None, Metering::Metered, true);
        assert_eq!(one.metered_units, 1);
        let b = BatchReceipt::part_way(
            vec![one.clone()],
            one.with_metered_units(2),
            crate::Error::Unreachable,
        );
        assert!(b.anything_sent());
        assert_eq!(b.metered_units(), 3);
        let partial = crate::Error::partial_send(1, 3, crate::Error::Unreachable);
        assert!(matches!(partial.root(), crate::Error::Unreachable));
        assert_eq!(
            partial.to_string(),
            "sent 1 of 3 messages, then: channel account is unreachable"
        );
    }

    #[test]
    fn reply_only_with_an_unexpired_token() {
        let t = ReplyToken {
            token: "r".into(),
            expires_at: Timestamp(100),
        };
        assert!(matches!(
            Delivery::select(Some(&t), Timestamp(99)),
            Delivery::Reply(_)
        ));
        assert!(matches!(
            Delivery::select(Some(&t), Timestamp(100)),
            Delivery::Push
        ));
        assert!(matches!(
            Delivery::select(None, Timestamp(0)),
            Delivery::Push
        ));
    }
}
