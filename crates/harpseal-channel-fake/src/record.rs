//! What the Fake Channel remembers, how a batch is recorded, and the
//! read-back API tests use.

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use harpseal::{
    Attachment, BatchReceipt, Capability, ChannelAccountId, DeleteMessage, Delivery, EditMessage,
    Error, Locale, Menu, Message, Metering, PlatformMessageId, Result, SendAttachment, SendReceipt,
};

use crate::FakeChannel;
use crate::hooks::Hook;
use crate::render::render;

/// `retry_after_secs` of the failure [`FakeChannel::fail_next_batch_at`]
/// injects.
pub const FAILURE_RETRY_AFTER_SECS: u32 = 30;

/// One object the Fake Channel showed a Student: a Message as the Core
/// built it, or one part of it after the profile's degrade rules.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Sent {
    /// Recipient.
    pub to: ChannelAccountId,
    /// How this object went out: the Core's Reply turns into Push past
    /// the profile's reply limit, once the token is spent, or on a profile
    /// without reply tokens.
    pub delivery: Delivery,
    /// The object as the Student saw it.
    pub message: Message,
    /// The id the Fake Channel gave it.
    pub id: PlatformMessageId,
    /// `Metered` only for the first object of a metered Push request: the
    /// Quota counts one unit per Push request to one recipient (a Message
    /// opening two Push requests has two `Metered` objects and costs 2).
    pub metering: Metering,
    /// Index of the Core Message it came from within its batch.
    pub source: usize,
}

/// One platform request the Fake Channel made (a reply call or a push
/// call), as a real adapter would send it over HTTP.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Call {
    /// Recipient.
    pub to: ChannelAccountId,
    /// The reply token spent, `None` for a Push.
    pub reply_token: Option<String>,
    /// Objects in the request.
    pub objects: usize,
    /// Whether the request counted against the Quota (once per recipient
    /// on a multicast).
    pub metering: Metering,
    /// Accounts the request went to: 1, or the group of a multicast
    /// (`to` is then the group's first account).
    pub recipients: usize,
}

#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) files: HashMap<String, Vec<u8>>,
    pub(crate) sent: Vec<Sent>,
    pub(crate) calls: Vec<Call>,
    pub(crate) used_tokens: HashSet<String>,
    pub(crate) menus: HashMap<ChannelAccountId, Option<Menu>>,
    pub(crate) edited: Vec<(PlatformMessageId, Message)>,
    pub(crate) deleted: Vec<PlatformMessageId>,
    /// Accounts whose sends fail with `Error::Unreachable`.
    pub(crate) blocked: HashSet<ChannelAccountId>,
    /// The next batch fails with `Error::RateLimited` before object `n`.
    pub(crate) fail_at: Option<usize>,
    /// When set, that armed failure is `Error::Platform(this)` instead.
    pub(crate) fail_with: Option<String>,
    /// The next edit or delete fails with `Error::RateLimited` after this
    /// many more succeed ([`FakeChannel::fail_next_maintenance_at`]).
    pub(crate) maint_fail_at: Option<usize>,
    /// How many edits or deletes in a row fail once it fires.
    pub(crate) maint_fails: usize,
    /// Runs once before the next batch or multicast is recorded.
    pub(crate) before_send: Option<Hook>,
    /// Idempotency keys of the batches sent ([`FakeChannel::keys`]).
    pub(crate) keys: Vec<String>,
    pub(crate) next_id: u64,
    /// Re-links and retires ([`crate::FakeChannel::menu_events`]).
    pub(crate) menu_events: Vec<crate::MenuEvent>,
    /// The next re-link stops after this many accounts.
    pub(crate) relink_fail_at: Option<usize>,
    /// [`crate::FakeChannel::set_relink_chunk`].
    pub(crate) relink_chunk: Option<usize>,
}

impl State {
    /// Disarms the armed failure and returns its error.
    pub(crate) fn take_failure(&mut self) -> Error {
        self.fail_at = None;
        match self.fail_with.take() {
            Some(m) => Error::Platform(m),
            None => Error::RateLimited {
                retry_after_secs: FAILURE_RETRY_AFTER_SECS,
            },
        }
    }
}

impl FakeChannel {
    /// Renders and records a batch. Objects (every rendered part counts)
    /// go into requests like a platform's: while the reply token is usable
    /// and the profile's reply limit is not reached, into the one reply
    /// request; the rest into Push requests of at most the profile's
    /// request size. A LINE-like token works once: a later batch with the
    /// same token is all Push.
    pub(crate) fn record_batch(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        msgs: &[Message],
        locale: &Locale,
    ) -> BatchReceipt {
        self.run_before_send();
        let profile = self.profile;
        let attachments_off = self.attachments_off();
        let mut s = self.lock();
        if s.blocked.contains(to) {
            return BatchReceipt::partial(Vec::new(), Error::Unreachable);
        }
        let token = match d {
            Delivery::Reply(t) if profile.reply_tokens() && !s.used_tokens.contains(&t.token) => {
                Some(t.token.clone())
            }
            _ => None,
        };
        let reply_limit = profile.reply_limit().unwrap_or(usize::MAX);
        let fail_at = s.fail_at;
        let mut objects = 0usize;
        let mut receipts = Vec::with_capacity(msgs.len());
        let mut reply_objects = 0usize;
        // Index into `s.calls` of the open Push request.
        let mut open_push: Option<usize> = None;
        for (source, msg) in msgs.iter().enumerate() {
            if matches!(msg, Message::Attachment(_)) && attachments_off {
                return BatchReceipt::partial(
                    receipts,
                    Error::Unsupported(Capability::SendAttachment),
                );
            }
            let mut parts = render(msg, profile.limits(), locale);
            if parts.is_empty() {
                parts.push(msg.clone());
            }
            let mut receipt = SendReceipt::new(None, Metering::Free, false);
            for (k, part) in parts.into_iter().enumerate() {
                if fail_at == Some(objects) {
                    let e = s.take_failure();
                    return match k {
                        0 => BatchReceipt::partial(receipts, e),
                        _ => BatchReceipt::part_way(receipts, receipt, e),
                    };
                }
                objects += 1;
                let (delivery, metering) = match &token {
                    Some(t) if reply_objects < reply_limit => {
                        if reply_objects == 0 {
                            s.used_tokens.insert(t.clone());
                            s.calls.push(Call {
                                to: to.clone(),
                                reply_token: Some(t.clone()),
                                objects: 0,
                                metering: Metering::Free,
                                recipients: 1,
                            });
                        }
                        let last = s.calls.len() - 1;
                        s.calls[last].objects += 1;
                        reply_objects += 1;
                        (d.clone(), Metering::Free)
                    }
                    _ => {
                        receipt.pushed = true;
                        let fits =
                            open_push.is_some_and(|i| s.calls[i].objects < profile.request_size());
                        let metering = if fits {
                            Metering::Free
                        } else {
                            let metering = if profile.push_metered() {
                                Metering::Metered
                            } else {
                                Metering::Free
                            };
                            s.calls.push(Call {
                                to: to.clone(),
                                reply_token: None,
                                objects: 0,
                                metering,
                                recipients: 1,
                            });
                            open_push = Some(s.calls.len() - 1);
                            metering
                        };
                        if let Some(i) = open_push {
                            s.calls[i].objects += 1;
                        }
                        (Delivery::Push, metering)
                    }
                };
                s.next_id += 1;
                let id = PlatformMessageId(format!("fake-{}", s.next_id));
                s.sent.push(Sent {
                    to: to.clone(),
                    delivery,
                    message: part,
                    id: id.clone(),
                    metering,
                    source,
                });
                if metering == Metering::Metered {
                    // One unit per Push request this Message opens.
                    receipt.add_metered_unit();
                }
                receipt.message_id = Some(id);
            }
            receipts.push(receipt);
        }
        BatchReceipt::complete(receipts)
    }

    /// Makes every later send and Menu change for `to` fail with
    /// `Error::Unreachable`, as a platform does once the Student blocked
    /// the bot (`false` undoes it).
    pub fn set_unreachable(&self, to: &ChannelAccountId, unreachable: bool) {
        let mut s = self.lock();
        if unreachable {
            s.blocked.insert(to.clone());
        } else {
            s.blocked.remove(to);
        }
    }

    /// Makes the next batch that reaches its object `part` (0-based,
    /// counting every rendered object) fail there with
    /// `Error::RateLimited` (retry after [`FAILURE_RETRY_AFTER_SECS`]): the
    /// objects before it go out and are reported as a partial
    /// [`BatchReceipt`]. Shorter batches go out whole and leave the failure
    /// armed; it fires once. On a multicast `part` counts groups of
    /// recipients instead: group `part` fails and the groups before it go
    /// out.
    pub fn fail_next_batch_at(&self, part: usize) {
        let mut s = self.lock();
        s.fail_at = Some(part);
        s.fail_with = None;
    }

    /// Like [`FakeChannel::fail_next_batch_at`], but the failure is
    /// `Error::Platform(message)`: not a "try later".
    pub fn fail_next_batch_at_with(&self, part: usize, message: &str) {
        let mut s = self.lock();
        s.fail_at = Some(part);
        s.fail_with = Some(message.to_string());
    }

    /// The idempotency key of every keyed batch so far, in order.
    pub fn keys(&self) -> Vec<String> {
        self.lock().keys.clone()
    }

    /// Every platform request so far, in order.
    pub fn calls(&self) -> Vec<Call> {
        self.lock().calls.clone()
    }
    /// Every platform request so far, and forgets them.
    pub fn take_calls(&self) -> Vec<Call> {
        std::mem::take(&mut self.lock().calls)
    }

    fn attachments_off(&self) -> bool {
        harpseal::Channel::attachments(self).is_none()
    }

    /// Every object sent so far, in order.
    pub fn sent(&self) -> Vec<Sent> {
        self.lock().sent.clone()
    }
    /// Every object sent so far, and forgets them.
    pub fn take_sent(&self) -> Vec<Sent> {
        std::mem::take(&mut self.lock().sent)
    }
    /// Everything one account saw, with Delivery and Metering, in order.
    pub fn seen_by(&self, to: &ChannelAccountId) -> Vec<Sent> {
        let s = self.lock();
        s.sent.iter().filter(|m| &m.to == to).cloned().collect()
    }
    /// The objects one account saw, in order.
    pub fn sent_to(&self, to: &ChannelAccountId) -> Vec<Message> {
        self.seen_by(to).into_iter().map(|m| m.message).collect()
    }
    /// The account's Menu: `None` if never set, `Some(None)` when locked.
    pub fn menu(&self, to: &ChannelAccountId) -> Option<Option<Menu>> {
        self.lock().menus.get(to).cloned()
    }
    /// Edits made through the EditMessage Capability.
    pub fn edited(&self) -> Vec<(PlatformMessageId, Message)> {
        self.lock().edited.clone()
    }
    /// Deletions made through the DeleteMessage Capability.
    pub fn deleted(&self) -> Vec<PlatformMessageId> {
        self.lock().deleted.clone()
    }
}

#[async_trait]
impl EditMessage for FakeChannel {
    async fn edit(
        &self,
        _: &ChannelAccountId,
        id: &PlatformMessageId,
        m: &Message,
        _: &Locale,
    ) -> Result<()> {
        let mut s = self.lock();
        s.maintenance()?;
        s.edited.push((id.clone(), m.clone()));
        Ok(())
    }
}

#[async_trait]
impl DeleteMessage for FakeChannel {
    async fn delete(&self, _: &ChannelAccountId, id: &PlatformMessageId) -> Result<()> {
        let mut s = self.lock();
        s.maintenance()?;
        s.deleted.push(id.clone());
        Ok(())
    }
}

#[async_trait]
impl SendAttachment for FakeChannel {
    async fn send_attachment(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        file: &Attachment,
        locale: &Locale,
    ) -> Result<SendReceipt> {
        let msg = [Message::Attachment(file.clone())];
        let mut receipts = self.record_batch(to, d, &msg, locale).into_result()?;
        receipts
            .pop()
            .ok_or_else(|| Error::Platform("fake: no receipt".into()))
    }
}

#[cfg(test)]
mod tests {
    use harpseal::{Error, Locale, Message};

    use crate::FakeChannel;

    #[test]
    fn an_injected_failure_waits_for_a_batch_long_enough() {
        let ch = FakeChannel::new("s").unwrap().telegram_like();
        let to = FakeChannel::account("u1");
        let push = harpseal::Delivery::Push;
        let texts =
            |n: usize| -> Vec<Message> { (0..n).map(|i| Message::text(format!("{i}"))).collect() };
        ch.fail_next_batch_at(2);
        let short = ch.record_batch(&to, &push, &texts(2), &Locale::th());
        assert!(short.error.is_none(), "too short to reach part 2");
        let long = ch.record_batch(&to, &push, &texts(3), &Locale::th());
        assert_eq!(long.receipts.len(), 2);
        assert!(matches!(long.error, Some(Error::RateLimited { .. })));
        let after = ch.record_batch(&to, &push, &texts(3), &Locale::th());
        assert!(after.error.is_none(), "it fires once");
    }
}
