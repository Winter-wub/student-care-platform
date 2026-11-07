//! Reply, Push and multicast. A handler's batch is rendered, then packed
//! like the Fake Channel's LineLike profile: the reply token works once and
//! covers the first 5 objects in one reply call (free); the rest go in Push
//! calls of at most 5 objects (`/v2/bot/message/push`), each one metered
//! unit, charged to the receipt of the Message that opens it (a Message
//! opening two Push requests costs 2 units). Every Push carries an
//! `X-Line-Retry-Key` derived from the Core's batch key ([`crate::retry_key`]).
//! A failed request ends the batch with the receipts of the Messages
//! wholly sent before it, plus the started Message as `unfinished`.
//! Limits: 5 objects per request
//! (<https://developers.line.biz/en/reference/messaging-api/#send-reply-message>,
//! <https://developers.line.biz/en/reference/messaging-api/#send-push-message>),
//! 500 recipients per multicast
//! (<https://developers.line.biz/en/reference/messaging-api/#send-multicast-message>).

use harpseal::{
    BatchReceipt, Capability, ChannelAccountId, Delivery, Error, Locale, Message, Metering,
    PlatformMessageId, Result, ResumeToken, SendReceipt,
};
use serde_json::{Value, json};

use crate::api::{API, Failure};
use crate::resume::{Resume, objects_digest};
use crate::retry_key::RetryKeys;
use crate::{LineChannel, render};

/// Most objects in one reply, push or multicast request.
pub const MESSAGES_PER_REQUEST: usize = render::MESSAGES_PER_REQUEST;
/// Most recipients in one multicast request.
pub const MULTICAST_MAX: usize = crate::multicast::MULTICAST_MAX;

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
    /// Sends a rendered batch. `key`: the Core's batch key, from which
    /// every Push request gets its `X-Line-Retry-Key` (none without it; a
    /// reply has no retry key). On a failed request, the Messages wholly
    /// sent before it are receipts, a Message whose first parts went out
    /// in earlier requests is `unfinished`, and the receipt carries a
    /// resume token ([`crate::resume`]) when a resume needs one to rebuild
    /// the failed request: `resume` is that token, handed back with the
    /// Messages from the stopped one on.
    pub(crate) async fn batch(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        msgs: &[Message],
        locale: &Locale,
        key: Option<&str>,
        resume: Option<&ResumeToken>,
    ) -> BatchReceipt {
        let Rendered {
            parts,
            messages,
            stop,
        } = render_batch(self, msgs, locale);
        let mut receipts = vec![SendReceipt::new(None, Metering::Free, false); messages];
        let token = d.reply_token().map(|t| t.token.as_str());
        let resume = resume.and_then(|t| applicable(t, &parts));
        // Parts of Message 0 an earlier attempt sent: not sent again.
        let skip = resume.as_ref().map_or(0, |r| r.skip);
        let mut keys = key.map(|k| match resume {
            Some(r) => RetryKeys::resumed(k, r.seen),
            None => RetryKeys::new(k),
        });
        for req in plan(parts.len() - skip, token) {
            let range = req.parts.start + skip..req.parts.end + skip;
            let chunk = &parts[range.clone()];
            let objects: Vec<&Value> = chunk.iter().map(|p| &p.object).collect();
            // The keys taken before this request: what a resume restores.
            let seen = keys.as_ref().map(|k| k.seen().clone()).unwrap_or_default();
            let sent = match &req.reply {
                Some(t) => match self.reply_call(t, &objects).await {
                    // Expired or spent token: the same objects go by Push.
                    Ok(Err(f)) if f.bad_reply_token() => {
                        self.push_call(to, &objects, keys.as_mut()).await
                    }
                    other => other.map(|r| {
                        r.map(|ids| Request {
                            ids,
                            pushed: false,
                            replayed: false,
                        })
                    }),
                },
                None => self.push_call(to, &objects, keys.as_mut()).await,
            };
            let sent = match sent {
                Ok(Ok(v)) => Ok(v),
                Ok(Err(f)) => Err(f.into_error(true)),
                Err(e) => Err(e),
            };
            let Request {
                ids,
                pushed,
                replayed,
            } = match sent {
                Ok(v) => v,
                Err(e) => {
                    // A Message whose parts went out before this request
                    // (now or on an earlier attempt) is `unfinished`.
                    let first = chunk[0].source;
                    let started = range.start > 0 && parts[range.start - 1].source == first;
                    let unfinished = started.then(|| receipts[first].clone());
                    // Messages wholly sent before this request.
                    receipts.truncate(first);
                    let opened = parts.iter().position(|p| p.source == first).unwrap_or(0);
                    let done: Vec<&Value> = parts[opened..range.start]
                        .iter()
                        .map(|p| &p.object)
                        .collect();
                    let token = (!done.is_empty() || !seen.is_empty()).then(|| {
                        Resume {
                            skip: done.len(),
                            skipped: objects_digest(&done),
                            seen,
                        }
                        .encode()
                    });
                    let out = match unfinished {
                        Some(u) => BatchReceipt::part_way(receipts, u, e),
                        None => BatchReceipt::partial(receipts, e),
                    };
                    return out.with_resume(token);
                }
            };
            for (i, p) in chunk.iter().enumerate() {
                let r = &mut receipts[p.source];
                if pushed {
                    r.pushed = true;
                    if i == 0 {
                        // One metered unit per Push request, charged to the
                        // Message that opens it; a 409 says the unit was
                        // spent on an earlier attempt.
                        r.add_metered_unit();
                        if replayed {
                            r.add_replayed_unit();
                        }
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

    /// A Push. A 409 for an already accepted retry key counts as sent (no
    /// ids, `replayed`).
    async fn push_call(
        &self,
        to: &ChannelAccountId,
        objects: &[&Value],
        keys: Option<&mut RetryKeys>,
    ) -> Result<std::result::Result<Request, Failure>> {
        let body = json!({ "to": to.external_id, "messages": objects });
        let retry = keys.map(|k| k.next(&[to.external_id.as_str()], objects));
        let got = self
            .post_json_keyed(
                &format!("{API}/v2/bot/message/push"),
                &body,
                retry.as_deref(),
            )
            .await?;
        Ok(got.map(|k| Request {
            ids: sent_ids(&k.body),
            pushed: true,
            replayed: k.replayed,
        }))
    }
}

/// The token's state when it fits these parts: its `skip` parts all belong
/// to Message 0 and are the objects it went out with. `None` otherwise (a
/// token from another batch, or not this adapter's): resume from the first
/// part, as without a token.
fn applicable(t: &ResumeToken, parts: &[Part]) -> Option<Resume> {
    let r = Resume::decode(t)?;
    let head = parts.get(..r.skip)?;
    let objects: Vec<&Value> = head.iter().map(|p| &p.object).collect();
    let fits = head.iter().all(|p| p.source == 0) && objects_digest(&objects) == r.skipped;
    fits.then_some(r)
}

/// One Reply or Push request that LINE accepted.
struct Request {
    /// `sentMessages[].id`, in order (empty after a 409).
    ids: Vec<Option<String>>,
    /// It was a Push.
    pushed: bool,
    /// LINE answered 409: it went out on an earlier attempt.
    replayed: bool,
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
