-- Kernel migration 0018 (#143): the Manual Link picker lists the chat
-- accounts with no live Link that wrote recently. It finds them from the
-- recent claimed webhook events and the recently created accounts, then
-- reads each account's newest event. Without these indexes every Student
-- page that opens the picker would read both tables whole, which D1 bills
-- per row.
CREATE INDEX IF NOT EXISTS inbound_event_sender
    ON inbound_event (channel, external_id, received_at);
CREATE INDEX IF NOT EXISTS channel_account_first_seen
    ON channel_account (first_seen_at);
