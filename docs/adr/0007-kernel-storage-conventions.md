---
status: accepted
date: 2026-11-06
---

# Kernel storage conventions: UUIDv7 text keys, millisecond timestamps, append-only audit

Every Kernel table (and, by convention, every Module table) uses a UUIDv7 stored as 36-character text as its primary key, generated in the Core from the Clock and Random ports. Natural keys such as the student ID stay unique columns, never primary keys. Timestamps are integer milliseconds since the Unix epoch in UTC; display converts to the Deployment's time zone. The audit log is append-only and records actor, action, target and the before/after JSON of every Staff write; no code path updates or deletes it.

We rejected integer auto-increment keys (D1 returns integers beyond ±2^53 inexactly and the Core never relies on a round-trip to the database for an id), ULIDs (shorter, but UUIDv7 is the form most tools and libraries already parse), natural primary keys (a changed student ID would cascade through every table), ISO-8601 text timestamps (range comparisons depend on exact formatting) and an audit log without before/after values (cannot answer "who changed this value from what").

## Consequences

- Ids are opaque and sortable by creation time; URLs and logs show UUIDs, not sequence numbers.
- The database holds no local time; every rendering layer must convert.
- Audit rows are never pruned; the retention clock applies to Outbound Message rows (default 90 days), not to audit, Consent or Security Event rows.
- Before/after JSON must not carry secrets: password hashes, OTP hashes and session tokens are redacted before writing.
