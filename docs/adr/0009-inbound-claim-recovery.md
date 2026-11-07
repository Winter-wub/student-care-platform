---
status: accepted
date: 2026-11-07
---

# An unfinished webhook claim is logged as lost, never taken over

The Kernel claims each webhook event id in `inbound_event` before handling the event (#35), and keeps the claim even when handling fails. If the process dies between the claim and the reply (a Worker past its CPU limit, a killed native process), every redelivery used to be skipped as a duplicate and nothing recorded that the event was lost. Now the claim has a `done_at`, set once the event was answered or logged as failed, and keeps the sender's external id and the event kind. A claim still unfinished 60 seconds after it was taken (`received_at`; `INBOUND_CLAIM_LEASE_MS`) is abandoned. Two mechanisms log it, each with one conditional write so it is logged once even when they race: the next redelivery of the event (a conditional `UPDATE` of that claim), and else `Deployment::tick` (one `UPDATE … RETURNING` over at most 100 abandoned claims per tick). Either sets `done_at` and writes one `inbound.lost` Security Event naming the Channel, the sender's external id, the event kind and id (never the payload). The event is never answered: a redelivery is answered as a duplicate, so the Student gets no second reply. `done_at` is set right after the handler returns, not in the handler's last batch: handlers end in different writes (a send, a Module's own statements).

Limitation: a redelivery that arrives within the 60 seconds is answered 200 as a plain duplicate (the first handler may still be running), so the platform stops retrying and that redelivery cannot log the loss. Telegram retries early, and LINE redelivery is off by default (a setting in the console), so the tick sweep is what catches most lost events; a redelivery after the lease only logs it sooner.

We rejected option (a), letting a redelivery take the claim over and handle the event again after a lease: the handler may have sent part of its answer before it died (Reply tokens spent, Pushes metered), so a takeover risks answering twice, which #35 judged worse than answering once or not at all. A lost event is not silent either way: Staff see it among the Security Events and the Student can ask again. The new kind `inbound.lost` is separate from `inbound.failed` (an event whose handling returned an error) because it means something else to Staff: the event may have been partly answered, or not at all, and nothing says which.

The lease is 60 seconds: beyond the platforms' webhook timeouts and a Worker's default CPU limit (30 s; configurable up to 5 minutes, and a Deployment that raises it should expect more spurious logs). A slow File copy (a large file downloaded from the platform and stored) is the expected reason a live handler exceeds it.

## Consequences

- Migration 0009 adds `inbound_event.done_at`, `external_id` and `kind`, and a partial index on unfinished claims; claims made before it count as finished.
- A process that dies after the reply but before `done_at` is written, or a `done_at` write that fails (best effort), makes a claim look abandoned: a spurious `inbound.lost` is logged though the event was answered.
- A handler that runs longer than 60 seconds is logged as lost by a redelivery or tick even though it may still finish and answer once.
- `TickReport::events_lost` counts the claims a tick logged; `WebhookReport` counts one found by a redelivery in `failed`, not in `duplicates`.
- Option (a) stays possible later with the same columns: it would re-handle where this decision logs.
