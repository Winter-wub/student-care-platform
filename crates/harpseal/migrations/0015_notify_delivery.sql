-- Kernel migration 0015: what a Module's Notice (#73) did per Student, so
-- the same key never sends a Student anything twice, even from two calls at
-- once. `notify_key` is `<module>:<key>`: two Modules' keys never meet.
--
-- A call claims a row before it sends (insert, or retake an `open` row or a
-- `sending` one whose 10-minute lease ran out) and goes on only when its
-- claim changed one row; it writes `done` (or `open` again, after a failure)
-- under its `claim_token`. `via` says what may be in flight: 'email' is set
-- before an email goes out, so a retaken row knows the provider may have
-- it (resent with the same idempotency key, or counted as sent when the
-- sender cannot de-duplicate); chat is found again by its
-- `outbound_message` rows (batch key `notify:<notify_key>:<student>`).
-- Rows are pruned 90 days after their last change.
CREATE TABLE notify_delivery (
    notify_key TEXT NOT NULL,
    student_id TEXT NOT NULL REFERENCES student (id),
    module TEXT NOT NULL,
    via TEXT CHECK (via IN ('chat', 'email')),
    status TEXT NOT NULL CHECK (status IN ('sending', 'done', 'open')),
    claim_token TEXT NOT NULL,
    claimed_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (notify_key, student_id)
);
CREATE INDEX notify_delivery_updated ON notify_delivery (updated_at);
