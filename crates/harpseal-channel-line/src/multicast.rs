//! Multicast: the same Messages to many accounts, at most
//! [`MULTICAST_MAX`] recipients per request
//! (<https://developers.line.biz/en/reference/messaging-api/#send-multicast-message>).
//! Each group of recipients gets the batch's Push requests (5 objects
//! each) in order; each request carries an `X-Line-Retry-Key` derived from
//! the batch key, the group and the objects, so a retry of a group with
//! the same key is not delivered twice.

use async_trait::async_trait;
use harpseal::{
    BatchReceipt, Capability, ChannelAccountId, Error, Locale, Message, Metering, Multicast,
    SendReceipt,
};
use serde_json::{Value, json};

use crate::LineChannel;
use crate::api::API;
use crate::retry_key::RetryKeys;
use crate::send::{Rendered, plan, render_batch};

/// Most recipients in one multicast request.
pub const MULTICAST_MAX: usize = 500;

#[async_trait]
impl Multicast for LineChannel {
    fn max_recipients(&self) -> usize {
        MULTICAST_MAX
    }

    /// Metering is per recipient: each recipient's receipt of the Message
    /// opening a request gets one unit per request it opens (N recipients
    /// in one request are N units). A failed request ends the send with
    /// the receipts of every recipient in the groups before it.
    ///
    /// A group whose first requests went out before a later one failed is
    /// not reported (no receipts, no `unfinished`): those Messages did
    /// reach its recipients, but nothing records them yet. A retry with the
    /// same key and Messages rebuilds those requests with the same retry
    /// keys; LINE answers 409, they count as sent, and their receipts and
    /// units are recorded then. Retry with a different key and they are
    /// delivered again.
    async fn send_many(
        &self,
        to: &[ChannelAccountId],
        msgs: &[Message],
        locale: &Locale,
        key: &str,
    ) -> BatchReceipt {
        if msgs.iter().any(|m| matches!(m, Message::Attachment(_))) {
            return BatchReceipt::partial(
                Vec::new(),
                Error::Unsupported(Capability::SendAttachment),
            );
        }
        if let Some(a) = to.iter().find(|a| a.channel != crate::KIND) {
            let e = Error::Invalid(format!("{} is not a LINE account", a.channel));
            return BatchReceipt::partial(Vec::new(), e);
        }
        let Rendered { parts, stop, .. } = render_batch(self, msgs, locale);
        if let Some(e) = stop {
            return BatchReceipt::partial(Vec::new(), e);
        }
        let requests = plan(parts.len(), None);
        // What one recipient's receipts look like once every request went.
        let mut one = vec![SendReceipt::new(None, Metering::Free, true); msgs.len()];
        for req in &requests {
            one[parts[req.parts.start].source].add_metered_unit();
        }
        let url = format!("{API}/v2/bot/message/multicast");
        let mut receipts = Vec::with_capacity(to.len() * msgs.len());
        let mut keys = RetryKeys::new(key);
        for group in to.chunks(MULTICAST_MAX) {
            let ids: Vec<&str> = group.iter().map(|a| a.external_id.as_str()).collect();
            for req in &requests {
                let objects: Vec<&Value> =
                    parts[req.parts.clone()].iter().map(|p| &p.object).collect();
                let body = json!({ "to": ids, "messages": objects });
                let retry = keys.next(&ids, &objects);
                let sent = match self.post_json_keyed(&url, &body, Some(&retry)).await {
                    Ok(Ok(_)) => Ok(()),
                    Ok(Err(f)) => Err(f.into_error(false)),
                    Err(e) => Err(e),
                };
                if let Err(e) = sent {
                    return BatchReceipt::partial(receipts, e);
                }
            }
            receipts.extend(group.iter().flat_map(|_| one.iter().cloned()));
        }
        BatchReceipt::complete(receipts)
    }
}
