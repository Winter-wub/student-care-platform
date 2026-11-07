-- Kernel migration 0009 (#44, ADR-0009): when the handling of a claimed
-- webhook event finished. NULL while it is being handled; set once it was
-- answered or logged as failed. A redelivery that finds a claim still NULL
-- after the lease (2 minutes) takes it as abandoned (the process died
-- between the claim and the reply): it sets `done_at`, logs one
-- `inbound.failed` Security Event, and is not answered. Claims made before
-- this migration are taken as finished.
ALTER TABLE inbound_event ADD COLUMN done_at INTEGER;
UPDATE inbound_event SET done_at = received_at WHERE done_at IS NULL;
