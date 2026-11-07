-- Kernel migration 0014: what a Module's notice (#73) did per Student, so a
-- retried Job with the same key sends nothing twice. Chat sends are also
-- found by their `outbound_message` rows (batch key `notify:<key>:<student>`);
-- this row is what makes an email at most once (email has no retry key).
-- `status` 'sending' is written before the send, 'done' after it: a chat
-- row left 'sending' is retried from its Outbound Message rows, an email row
-- left 'sending' counts as emailed (never sent twice).
CREATE TABLE notify_delivery (
    notify_key TEXT NOT NULL,
    student_id TEXT NOT NULL REFERENCES student (id),
    module TEXT NOT NULL,
    via TEXT NOT NULL CHECK (via IN ('chat', 'email')),
    status TEXT NOT NULL CHECK (status IN ('sending', 'done')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (notify_key, student_id)
);
