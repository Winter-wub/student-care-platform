//! Reply, Push and multicast. A handler's batch is rendered, then packed
//! like the Fake Channel's LineLike profile: the reply token works once and
//! covers the first 5 objects in one reply call (free); the rest go in Push
//! calls of at most 5 objects (`/v2/bot/message/push`), each one metered
//! unit, reported on the receipt of the Message that opens it.
//! Limits: 5 objects per request
//! (<https://developers.line.biz/en/reference/messaging-api/#send-reply-message>,
//! <https://developers.line.biz/en/reference/messaging-api/#send-push-message>),
//! 500 recipients per multicast
//! (<https://developers.line.biz/en/reference/messaging-api/#send-multicast-message>).

use async_trait::async_trait;
use harpseal::{
    BatchReceipt, Capability, ChannelAccountId, Delivery, Error, Locale, Message, Metering,
    Multicast, PlatformMessageId, Result, SendReceipt,
};
use serde_json::{Value, json};

use crate::api::{API, Failure};
use crate::{LineChannel, render};

/// Most objects in one reply, push or multicast request.
pub const MESSAGES_PER_REQUEST: usize = render::MESSAGES_PER_REQUEST;
/// Most recipients in one multicast request.
pub const MULTICAST_MAX: usize = 500;

/// One rendered object and the index of the Message it came from.
#[derive(Debug, Clone)]
pub(crate) struct Part {
    /// Index of the source Message in the batch.
    pub(crate) source: usize,
    /// The LINE message object.
    pub(crate) object: Value,
}

/// One planned request: `reply` spends the token, otherwise a Push.
#[derive(Debug, Clone)]
pub(crate) struct Planned {
    /// The reply token, `None` for a Push.
    pub(crate) reply: Option<String>,
    /// Range into the parts.
    pub(crate) parts: std::ops::Range<usize>,
}

/// What [`render_batch`] produced.
pub(crate) struct Rendered {
    pub(crate) parts: Vec<Part>,
    /// Messages rendered, in order, before `stop`.
    pub(crate) messages: usize,
    /// Why rendering stopped early: an Attachment (LINE has no
    /// SendAttachment Capability) or a Message that renders to nothing
    /// (e.g. an empty CardList), which must not get a receipt.
    pub(crate) stop: Option<Error>,
}

pub(crate) fn render_batch(ch: &LineChannel, msgs: &[Message], locale: &Locale) -> Rendered {
    let cx = ch.render_cx(locale);
    let mut parts = Vec::new();
    for (source, m) in msgs.iter().enumerate() {
        let stop = |e| Rendered {
            parts: Vec::new(),
            messages: source,
            stop: Some(e),
        };
        if matches!(m, Message::Attachment(_)) {
            return Rendered {
                parts,
                ..stop(Error::Unsupported(Capability::SendAttachment))
            };
        }
        let objects = render::message(m, &cx);
        if objects.is_empty() {
            let e = Error::Invalid(format!(
                "a {} that LINE cannot show (nothing to send)",
                m.kind()
            ));
            return Rendered { parts, ..stop(e) };
        }
        parts.extend(objects.into_iter().map(|object| Part { source, object }));
    }
    Rendered {
        parts,
        messages: msgs.len(),
        stop: None,
    }
}

/// Packs `n` objects: the first 5 into the reply when there is a token,
/// the rest into Pushes of 5.
pub(crate) fn plan(n: usize, reply: Option<&str>) -> Vec<Planned> {
    let mut out = Vec::new();
    let mut start = 0;
    if let Some(t) = reply.filter(|_| n > 0) {
        start = n.min(MESSAGES_PER_REQUEST);
        out.push(Planned {
            reply: Some(t.to_string()),
            parts: 0..start,
        });
    }
    while start < n {
        let end = n.min(start + MESSAGES_PER_REQUEST);
        out.push(Planned {
            reply: None,
            parts: start..end,
        });
        start = end;
    }
    out
}

/// LINE meters Push only.
pub fn metering(d: &Delivery) -> Metering {
    match d {
        Delivery::Reply(_) => Metering::Free,
        _ => Metering::Metered,
    }
}

impl LineChannel {
    pub(crate) async fn batch(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        msgs: &[Message],
        locale: &Locale,
    ) -> BatchReceipt {
        let Rendered {
            parts,
            messages,
            stop,
        } = render_batch(self, msgs, locale);
        let mut receipts = vec![SendReceipt::new(None, Metering::Free, false); messages];
        let token = d.reply_token().map(|t| t.token.as_str());
        for req in plan(parts.len(), token) {
            let chunk = &parts[req.parts.clone()];
            let objects: Vec<&Value> = chunk.iter().map(|p| &p.object).collect();
            let sent = match &req.reply {
                Some(t) => match self.reply_call(t, &objects).await {
                    // Expired or spent token: the same objects go by Push.
                    Ok(Err(f)) if f.bad_reply_token() => self.push_call(to, &objects).await,
                    other => other.map(|r| r.map(|ids| (ids, false))),
                },
                None => self.push_call(to, &objects).await,
            };
            let sent = match sent {
                Ok(Ok(v)) => Ok(v),
                Ok(Err(f)) => Err(f.into_error(true)),
                Err(e) => Err(e),
            };
            let (ids, pushed) = match sent {
                Ok(v) => v,
                Err(e) => {
                    // Messages wholly sent before this request.
                    receipts.truncate(chunk[0].source);
                    return BatchReceipt::partial(receipts, e);
                }
            };
            for (i, p) in chunk.iter().enumerate() {
                let r = &mut receipts[p.source];
                if pushed {
                    r.pushed = true;
                    if i == 0 {
                        r.metering = Metering::Metered;
                    }
                }
                if let Some(id) = ids.get(i).cloned().flatten() {
                    r.message_id = Some(PlatformMessageId(id));
                }
            }
        }
        match stop {
            Some(e) => BatchReceipt::partial(receipts, e),
            None => BatchReceipt::complete(receipts),
        }
    }

    async fn reply_call(
        &self,
        token: &str,
        objects: &[&Value],
    ) -> Result<std::result::Result<Vec<Option<String>>, Failure>> {
        let body = json!({ "replyToken": token, "messages": objects });
        let got = self
            .post_json(&format!("{API}/v2/bot/message/reply"), &body)
            .await?;
        Ok(got.map(|v| sent_ids(&v)))
    }

    /// A Push: `(ids, true)`.
    async fn push_call(
        &self,
        to: &ChannelAccountId,
        objects: &[&Value],
    ) -> Result<std::result::Result<(Vec<Option<String>>, bool), Failure>> {
        let body = json!({ "to": to.external_id, "messages": objects });
        let got = self
            .post_json(&format!("{API}/v2/bot/message/push"), &body)
            .await?;
        Ok(got.map(|v| (sent_ids(&v), true)))
    }
}

/// `sentMessages[].id` of a reply or push answer, in order.
fn sent_ids(v: &Value) -> Vec<Option<String>> {
    v["sentMessages"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|m| m["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl Multicast for LineChannel {
    fn max_recipients(&self) -> usize {
        MULTICAST_MAX
    }

    async fn send_many(
        &self,
        to: &[ChannelAccountId],
        msgs: &[Message],
        locale: &Locale,
    ) -> Result<Vec<SendReceipt>> {
        if msgs.iter().any(|m| matches!(m, Message::Attachment(_))) {
            return Err(Error::Unsupported(Capability::SendAttachment));
        }
        if let Some(a) = to.iter().find(|a| a.channel != crate::KIND) {
            return Err(Error::Invalid(format!(
                "{} is not a LINE account",
                a.channel
            )));
        }
        let Rendered { parts, stop, .. } = render_batch(self, msgs, locale);
        if let Some(e) = stop {
            return Err(e);
        }
        let requests = plan(parts.len(), None);
        let url = format!("{API}/v2/bot/message/multicast");
        for group in to.chunks(MULTICAST_MAX) {
            let ids: Vec<&str> = group.iter().map(|a| a.external_id.as_str()).collect();
            for req in &requests {
                let objects: Vec<&Value> =
                    parts[req.parts.clone()].iter().map(|p| &p.object).collect();
                let body = json!({ "to": ids, "messages": objects });
                self.post_json(&url, &body)
                    .await?
                    .map_err(|f| f.into_error(false))?;
            }
        }
        // Per recipient: the Message opening each request is metered.
        let mut one = vec![SendReceipt::new(None, Metering::Free, true); msgs.len()];
        for req in &requests {
            one[parts[req.parts.start].source].metering = Metering::Metered;
        }
        Ok(to.iter().flat_map(|_| one.iter().cloned()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(p: &[Planned]) -> Vec<(bool, usize)> {
        p.iter()
            .map(|r| (r.reply.is_some(), r.parts.len()))
            .collect()
    }

    #[test]
    fn a_token_covers_five_objects_then_pushes_of_five() {
        assert_eq!(
            shape(&plan(12, Some("t"))),
            [(true, 5), (false, 5), (false, 2)]
        );
        assert_eq!(shape(&plan(3, Some("t"))), [(true, 3)]);
        assert_eq!(shape(&plan(7, None)), [(false, 5), (false, 2)]);
        assert!(plan(0, Some("t")).is_empty());
    }
}
