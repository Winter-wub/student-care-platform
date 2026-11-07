---
status: accepted
date: 2026-11-06
---

# Kernel storage conventions: UUIDv7 text keys, millisecond timestamps, append-only audit

Every Kernel table (and, by convention, every Module table) uses a UUIDv7 stored as 36-character text as its primary key, generated in the Core from the Clock and Random ports. Natural keys such as the student ID stay unique columns, never primary keys. Timestamps are integer milliseconds since the Unix epoch in UTC; display converts to the Deployment's time zone. The audit log is append-only and records actor, action, target and the before/after JSON of every Staff write; no code path updates or deletes it.

We rejected integer auto-increment keys (D1 returns integers beyond ±2^53 inexactly and the Core never relies on a round-trip to the database for an id), ULIDs (shorter, but UUIDv7 is the form most tools and libraries already parse), natural primary keys (a changed student ID would cascade through every table), ISO-8601 text timestamps (range comparisons depend on exact formatting) and an audit log without before/after values (cannot answer "who changed this value from what").

Amended 2026-11-06 (#14 review): join tables (`student_tag`, `role_permission`, `staff_role`), aggregate tables (`usage_daily` by channel and day) and keyed tables (`setting` by key and Locale, `migration` by Module and name) use their natural composite key as primary key, and session tables (`staff_session`, `app_session`) use the SHA-256 hash of the token as `id`; every other table keeps a UUIDv7. UUIDv7 ids sort by creation time only to the millisecond (on Workers, to the request, since the clock is frozen within one), so lists order by `(at, id)`, never by `id` alone.

Amended 2026-11-07 by ADR-0008: `restore --force` is the one deleting path; it replaces the audit log with the backup's and records what it deleted in its `backup.restore` audit row.

## Consequences

- Ids are opaque and sortable by creation time; URLs and logs show UUIDs, not sequence numbers.
- The database holds no local time; every rendering layer must convert.
- Audit rows are never pruned; the retention clock applies to Outbound Message rows (default 90 days), not to audit, Consent or Security Event rows.
- Before/after JSON must not carry secrets: password hashes, OTP hashes and session tokens are redacted before writing.
