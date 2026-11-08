//! The Multicast Capability of the LineLike profile: one Push request per
//! group of recipients (and per 5 objects), metered once per recipient.

use async_trait::async_trait;
use harpseal::{
    BatchReceipt, ChannelAccountId, Delivery, Locale, Message, Metering, Multicast,
    PlatformMessageId, SendReceipt,
};

use crate::record::{Call, Sent};
use crate::render::{refused, refused_url, render};
use crate::{FakeChannel, FakeProfile};

/// Recipients per multicast request by default (LINE's limit).
pub(crate) const MAX_RECIPIENTS: usize = 500;

#[async_trait]
impl Multicast for FakeChannel {
    fn max_recipients(&self) -> usize {
        self.max_recipients
    }

    /// Splits `to` into groups of [`Multicast::max_recipients`]. Each group
    /// opens one [`Call`] per Push request (5 objects each), with
    /// `recipients` set; every recipient gets its own [`Sent`] objects and
    /// receipts (recipient-major), the first object of each request
    /// metered for each recipient. Blocked accounts are not reported (LINE
    /// does not say which recipient failed). An armed
    /// [`FakeChannel::fail_next_batch_at`] fails group `n`: the groups
    /// before it are the receipts, and the error is set.
    async fn send_many(
        &self,
        to: &[ChannelAccountId],
        msgs: &[Message],
        locale: &Locale,
        key: &str,
    ) -> BatchReceipt {
        self.run_before_send();
        if msgs.iter().any(refused_url) {
            return BatchReceipt::partial(Vec::new(), refused());
        }
        let limits = self.profile.limits();
        let size = self.profile.request_size();
        let mut s = self.lock();
        s.keys.push(key.to_string());
        // Rendered once: (source Message, object), in order.
        let objects: Vec<(usize, Message)> = msgs
            .iter()
            .enumerate()
            .flat_map(|(i, m)| {
                let mut parts = render(m, limits, locale);
                if parts.is_empty() {
                    parts.push(m.clone());
                }
                parts.into_iter().map(move |p| (i, p))
            })
            .collect();
        let mut receipts = Vec::with_capacity(to.len() * msgs.len());
        let metering = if self.profile == FakeProfile::LineLike {
            Metering::Metered
        } else {
            Metering::Free
        };
        for (g, group) in to.chunks(self.max_recipients).enumerate() {
            if s.fail_at == Some(g) {
                let e = s.take_failure();
                return BatchReceipt::partial(receipts, e);
            }
            for chunk in objects.chunks(size.max(1)) {
                s.calls.push(Call {
                    to: group[0].clone(),
                    reply_token: None,
                    objects: chunk.len(),
                    metering,
                    recipients: group.len(),
                });
            }
            for account in group {
                let mut mine: Vec<SendReceipt> = (0..msgs.len())
                    .map(|_| SendReceipt::new(None, Metering::Free, true))
                    .collect();
                for (k, (source, part)) in objects.iter().enumerate() {
                    s.next_id += 1;
                    let id = PlatformMessageId(format!("fake-{}", s.next_id));
                    let opens = k % size.max(1) == 0;
                    let m = if opens { metering } else { Metering::Free };
                    if m == Metering::Metered {
                        mine[*source].add_metered_unit();
                    }
                    mine[*source].message_id = Some(id.clone());
                    s.sent.push(Sent {
                        to: account.clone(),
                        delivery: Delivery::Push,
                        message: part.clone(),
                        id,
                        metering: m,
                        source: *source,
                    });
                }
                receipts.extend(mine);
            }
        }
        BatchReceipt::complete(receipts)
    }
}
