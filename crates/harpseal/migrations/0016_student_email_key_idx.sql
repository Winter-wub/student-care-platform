-- Kernel migration 0016 (#71): the chat claim looks for another Student
-- with the same address (`lower(trim(email))`, `flow/claim.rs`) on every
-- claim and resend; without this index each one reads the whole student
-- table, which D1 bills per row.
CREATE INDEX IF NOT EXISTS student_email_key_idx ON student (lower(trim(email)));
