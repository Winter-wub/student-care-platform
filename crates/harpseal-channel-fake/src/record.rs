//! What the Fake Channel remembers, how a batch is recorded, and the
//! read-back API tests use.

use std::collections::HashMap;

use async_trait::async_trait;
use harpseal::{
    Attachment, BatchReceipt, Capability, ChannelAccountId, DeleteMessage, Delivery, EditMessage,
    Error, Locale, Menu, Message, Metering, PlatformMessageId, Result, SendAttachment, SendReceipt,
};

use crate::FakeChannel;
use crate::render::render;

/// One object the Fake Channel showed a Student: a Message as the Core
/// built it, or one part of it after the profile's degrade rules.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Sent {
    /// Recipient.
    pub to: ChannelAccountId,
    /// How this object went out: the Core's Reply turns into Push past
    /// the profile's reply limit, or on a profile without reply tokens.
    pub delivery: Delivery,
    /// The object as the Student saw it.
    pub message: Message,
    /// The id the Fake Channel gave it.
    pub id: PlatformMessageId,
    /// Whether it counted against the Quota.
    pub metering: Metering,
    /// Index of the Core Message it came from within its batch.
    pub source: usize,
}

#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) files: HashMap<String, Vec<u8>>,
    pub(crate) sent: Vec<Sent>,
    pub(crate) menus: HashMap<ChannelAccountId, Option<Menu>>,
    pub(crate) edited: Vec<(PlatformMessageId, Message)>,
    pub(crate) deleted: Vec<PlatformMessageId>,
    next_id: u64,
}

impl FakeChannel {
    /// Renders and records a batch: object `n` of the batch (counting
    /// every rendered part) uses the reply token while `n` is below the
    /// profile's reply limit; the rest are Pushes.
    pub(crate) fn record_batch(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        msgs: &[Message],
        locale: &Locale,
    ) -> BatchReceipt {
        let profile = self.profile;
        let mut s = self.lock();
        let mut receipts = Vec::with_capacity(msgs.len());
        let mut objects = 0usize;
        for (source, msg) in msgs.iter().enumerate() {
            if matches!(msg, Message::Attachment(_)) && self.attachments_off() {
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
            for part in parts {
                let reply_ok = profile.reply_tokens()
                    && profile.reply_limit().is_none_or(|limit| objects < limit);
                let delivery = match d {
                    Delivery::Reply(_) if reply_ok => d.clone(),
                    _ => Delivery::Push,
                };
                let push = delivery.reply_token().is_none();
                let metering = if push && profile.push_metered() {
                    Metering::Metered
                } else {
                    Metering::Free
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
                    receipt.metering = Metering::Metered;
                }
                receipt.pushed |= push;
                receipt.message_id = Some(id);
                objects += 1;
            }
            receipts.push(receipt);
        }
        BatchReceipt::complete(receipts)
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
        self.lock().edited.push((id.clone(), m.clone()));
        Ok(())
    }
}

#[async_trait]
impl DeleteMessage for FakeChannel {
    async fn delete(&self, _: &ChannelAccountId, id: &PlatformMessageId) -> Result<()> {
        self.lock().deleted.push(id.clone());
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
