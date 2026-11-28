-- Announcements Module: asking for a new acknowledgement of an edit.
--
-- Staff editing a sent announcement that asks for an acknowledgement may
-- tick "ขอให้รับทราบใหม่": everyone acknowledges the edited version again.
-- The save, in its gated batch, keeps the closing version here (its texts,
-- who was in the Audience and who acknowledged, and when), clears
-- `announcement_delivery.acked_at` and raises `announcement.ack_round`.
-- The counts on the pages, the Menu and the Mini App then read the latest
-- version only, as before; the history reads these tables.

-- The version Students acknowledge now: 0 for the first, +1 per request.
ALTER TABLE announcement ADD COLUMN ack_round INTEGER NOT NULL DEFAULT 0;

-- One row per closed version.
CREATE TABLE announcement_ack_round (
    announcement_id TEXT NOT NULL REFERENCES announcement (id),
    ack_round INTEGER NOT NULL,
    closed_at INTEGER NOT NULL,
    closed_by TEXT REFERENCES staff (id),
    -- The texts Students acknowledged: [{"locale", "title", "body"}].
    texts TEXT NOT NULL CHECK (json_valid(texts)),
    -- Distinct Students with a delivery row, and those who acknowledged.
    students INTEGER NOT NULL,
    acked INTEGER NOT NULL,
    PRIMARY KEY (announcement_id, ack_round)
);

-- Who acknowledged a closed version, and when (their first tap).
CREATE TABLE announcement_ack_history (
    announcement_id TEXT NOT NULL REFERENCES announcement (id),
    ack_round INTEGER NOT NULL,
    student_id TEXT NOT NULL REFERENCES student (id),
    acked_at INTEGER NOT NULL,
    PRIMARY KEY (announcement_id, ack_round, student_id)
);
