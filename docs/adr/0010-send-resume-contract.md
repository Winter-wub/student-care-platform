---
status: accepted
date: 2026-11-07
---

# A resume hands back the whole batch, its place in it, and the adapter's token

Until #62 a batch that stopped part-way (`Error::PartialSend { sent, .. }`) was resumed by sending `msgs[sent..]` with the same key, and the Kernel guessed what to meter from `SendReceipt::replayed_units` (#44). That guess over-counted a Message replayed whole after a lost answer and under-counted when the write recording a partial send failed. It also could not stop a double delivery on LINE: LINE packs the parts of several Messages into one request of 5 objects, so a resume from the stopped Message built different requests with different retry keys, and LINE delivered again the parts it already had. Worse, the retry key's occurrence number restarted with the resume, so a resumed request identical to an earlier accepted one (ten identical texts) got that request's key and LINE silently dropped it.

The contract is now:

- `Deployment::send_batch_with(to, reply, msgs, locale, &SendOptions)` always takes the whole batch. `SendOptions::key` sets the idempotency key; `SendOptions::resume_after(&err)` makes the send start at Message `sent` and carry the adapter's token. `PartialSend::sent` counts over the whole batch again, so the same call resumes any number of times. `send_batch_with_key` stays as the keyed send of a whole batch.
- `Error::PartialSend` and `BatchReceipt` gain `resume: Option<ResumeToken>`, an opaque string the adapter writes and reads back in the new `Channel::send_batch_resumed` (default: ignore it and call `send_batch_keyed`). An adapter must ignore a token it cannot read or that does not fit the Messages, and resume as without one.
- The LINE token holds how many parts of the stopped Message were accepted (with a digest of their objects) and the occurrence counts of the retry keys already used. The resume renders from the stopped Message, drops those parts and packs the rest from the failed request on: the same objects, the same keys. Telegram and the Fake Channel give no token: they send one part per request and have no retry keys, so the parts of the stopped Message already delivered go again, as before.
- Every `outbound_message` row records its Message's index in the batch and the units counted for it (migration 0010). A keyed send reads the rows of its key first; a unit sent now always counts, a replayed unit (409) counts only beyond what the row records. Metering is exact whether the earlier attempt's answer, its write, or both were lost.

We kept both mechanisms, as the ticket allowed either: the rows make metering exact for every way of retrying (a whole resend, a resume with or without a token, any Channel), and the token is what keeps a LINE resume from delivering twice. The token alone could not fix metering after a failed write (nothing would say what was recorded), and the rows alone could not make LINE rebuild a request it had packed with earlier Messages.

We rejected storing the token in the database: it lives as long as the caller's error value. A caller that cannot keep it (a Job retried in a later run) resends the whole batch with the same key, which LINE de-duplicates request by request and the rows meter exactly.

## Consequences

- Public API additions: `SendOptions`, `Deployment::send_batch_with`, `ResumeToken`, `Channel::send_batch_resumed`, `BatchReceipt::resume` and `BatchReceipt::with_resume`, `Error::resume_token`. `Error::PartialSend` gains the field `resume`: code that builds it, or matches it without `..`, must add it.
- Calling `send_batch_with_key` with `msgs[sent..]`, the old way to resume, still sends the right Messages but records them at the wrong indices: their rows may not be found by a later resend. Resume with `SendOptions::resume_after`.
- A keyed send costs one more query (none for an unkeyed one: its fresh key has no rows). Two sends with the same key at once are not supported: each reads before it writes.
- Rows written before migration 0010 have no index and are never matched: a resume across the upgrade meters as a fresh send.
- Multicast rows record their index and units too; their batch key stays NULL and the per-recipient resume rule of `Multicast::send_many` is unchanged.
