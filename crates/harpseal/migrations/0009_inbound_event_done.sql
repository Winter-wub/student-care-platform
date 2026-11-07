-- Kernel migration 0009 (#44, ADR-0009).
--
-- `inbound_event.done_at`: when the handling of a claimed webhook event
-- finished. NULL while it is being handled; set once it was answered or
-- logged as failed. A claim still NULL 60 seconds after `received_at` is
-- abandoned (the process died between the claim and the reply): the next
-- redelivery of the event, or else the tick, sets `done_at` and logs one
-- `inbound.lost` Security Event; the event is never answered. The sender's
-- `external_id` and the event `kind` are kept on the claim so the tick can
-- name them. Claims made before this migration are taken as finished.
--
-- `outbound_message.batch_key`: the idempotency key of the batch that sent
-- the row. A resume after a partial send completes the "partly sent" row
-- of the same batch key instead of writing a second row. NULL on
-- multicast rows and on rows written before this migration.
ALTER TABLE inbound_event ADD COLUMN done_at INTEGER;
ALTER TABLE inbound_event ADD COLUMN external_id TEXT;
ALTER TABLE inbound_event ADD COLUMN kind TEXT;
UPDATE inbound_event SET done_at = received_at WHERE done_at IS NULL;
CREATE INDEX IF NOT EXISTS inbound_event_open ON inbound_event (received_at)
    WHERE done_at IS NULL;
ALTER TABLE outbound_message ADD COLUMN batch_key TEXT;
