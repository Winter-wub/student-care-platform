-- Kernel migration 0010 (#62).
--
-- `outbound_message.msg_index`: the position of the Message in its batch
-- (0 for the first Message given to `Deployment::send_batch*`, counted
-- over the whole batch on a resume: `SendOptions::resume_after` keeps it).
-- `outbound_message.units`: the metered units recorded for the row so far
-- (`usage_daily.metered` holds their sum). A resume with the same batch
-- key reads the rows of that key to know exactly which Messages and units
-- an earlier attempt recorded: a Push answered 409 adds a unit only when
-- no row recorded it yet (the answer of the attempt that sent it, or the
-- write recording it, was lost). NULL on rows written before this
-- migration; such rows are never matched by a resume.
ALTER TABLE outbound_message ADD COLUMN msg_index INTEGER;
ALTER TABLE outbound_message ADD COLUMN units INTEGER;
CREATE INDEX IF NOT EXISTS outbound_message_batch ON outbound_message (batch_key, msg_index)
    WHERE batch_key IS NOT NULL;
