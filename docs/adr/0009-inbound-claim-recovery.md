---
status: accepted
date: 2026-11-07
---

# An unfinished webhook claim is logged on redelivery, never taken over

The Kernel claims each webhook event id in `inbound_event` before handling the event (#35), and keeps the claim even when handling fails. If the process dies between the claim and the reply (a Worker past its CPU limit, a killed native process), every redelivery used to be skipped as a duplicate and nothing recorded that the event was lost. Now the claim has a `done_at`, set once the event was answered or logged as failed. A redelivery that finds a claim still unfinished more than 60 seconds after it was taken (`received_at`; `INBOUND_CLAIM_LEASE_MS`) marks it done with one conditional `UPDATE` (so only one redelivery wins), writes one `inbound.failed` Security Event naming the Channel Account and the event id (never the payload), and is answered as a duplicate: the Student gets no second reply. Within the 60 seconds the redelivery is a plain duplicate, since the first handler may still be running. `done_at` is set right after the handler returns, not in the handler's last batch: handlers end in different writes (a send, a Module's own statements), and a claim marked done a moment late only risks the spurious log below.

We rejected letting a redelivery take the claim over and handle the event again after a lease: the handler may have sent part of its answer before it died (Reply tokens spent, Pushes metered), so a takeover risks answering twice, which #35 judged worse than answering once or not at all. A failed event is not lost silently either way: Staff see it among the Security Events and the Student can ask again. The event keeps the existing kind `inbound.failed` rather than a new `inbound.lost`: Staff already filter and read that kind, and a new kind would need its own back-office label; the error text says the handling never finished. We also rejected a tick that sweeps unfinished claims without a redelivery: it would need the sender and event kind stored on every claim. Redelivery is optional on LINE (a setting in the console), so with it off a killed handler still leaves no record. Revisit if that shows up in practice.

## Consequences

- Migration 0009 adds `inbound_event.done_at`; claims made before it count as finished.
- A `done_at` write that fails (best effort) makes a later redelivery log a spurious `inbound.failed`; it still does not answer.
- A handler that runs longer than 60 seconds and is then redelivered is logged as failed even though it may still finish; it may then answer once, as it would have anyway.
- Option (a), a takeover after the lease, stays possible later with the same column: it would re-handle where this decision logs.
