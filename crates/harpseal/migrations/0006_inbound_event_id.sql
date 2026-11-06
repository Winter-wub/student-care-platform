-- Kernel migration 0006 (#35): webhook events already handled, so a
-- platform retry (LINE `webhookEventId`, Telegram `update_id`) is answered
-- once. The Kernel claims `(channel, event_id)` with one conditional
-- INSERT before handling the event; a second delivery finds the row and
-- is ignored. A separate table rather than a column on `inbound_message`:
-- that table holds only text and file rows of known accounts, written
-- while the event is handled, and every event kind needs the claim first.
-- Rows older than `INBOUND_EVENT_DAYS` (7) are deleted after each webhook
-- call; platforms stop retrying long before that.
CREATE TABLE IF NOT EXISTS inbound_event (
    channel TEXT NOT NULL,
    event_id TEXT NOT NULL,
    received_at INTEGER NOT NULL,
    PRIMARY KEY (channel, event_id)
);
CREATE INDEX IF NOT EXISTS inbound_event_received ON inbound_event (received_at);
