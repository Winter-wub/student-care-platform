-- Announcements Module (#10, #25). Times are INTEGER ms UTC; ids UUIDv7
-- (ADR-0007). Recorded in `migration` under module 'announcements'.

CREATE TABLE announcement (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN (
        'draft', 'scheduled', 'sending', 'sent', 'retracted')),
    -- harpseal::audience::Audience as JSON.
    audience TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(audience)),
    send_at INTEGER, -- NULL: send at once when confirmed
    expires_at INTEGER, -- hidden from lists from then on; acks still accepted
    require_ack INTEGER NOT NULL DEFAULT 0 CHECK (require_ack IN (0, 1)),
    created_by TEXT REFERENCES staff (id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    sent_at INTEGER, -- the last round finished
    retracted_at INTEGER,
    -- Delivery round: 0 for the first send, +1 per resend.
    round INTEGER NOT NULL DEFAULT 0,
    -- The round whose Audience is in announcement_delivery (-1: none yet).
    snapshot_round INTEGER NOT NULL DEFAULT -1,
    -- +1 on every change: the edit form's version token and part of the
    -- send Job's key (a schedule cancelled and confirmed again gets a new Job).
    version INTEGER NOT NULL DEFAULT 0,
    -- The last edit's EditMessage outcome: {"at", "edited", "failed"}.
    edit_report TEXT CHECK (edit_report IS NULL OR json_valid(edit_report))
);
CREATE INDEX announcement_status ON announcement (status, created_at);

CREATE TABLE announcement_text (
    announcement_id TEXT NOT NULL REFERENCES announcement (id),
    locale TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY (announcement_id, locale)
);

CREATE TABLE announcement_delivery (
    id TEXT PRIMARY KEY,
    announcement_id TEXT NOT NULL REFERENCES announcement (id),
    round INTEGER NOT NULL,
    student_id TEXT NOT NULL REFERENCES student (id),
    link_id TEXT NOT NULL REFERENCES link (id),
    channel TEXT NOT NULL,
    channel_account_id TEXT NOT NULL REFERENCES channel_account (id),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN (
        'pending', 'sent', 'failed', 'skipped')),
    platform_message_id TEXT,
    sent_at INTEGER,
    acked_at INTEGER,
    error TEXT, -- an error kind, never platform text
    -- After a retract: 'deleted', 'too_old' (past the Channel's delete
    -- window) or 'failed'; NULL otherwise.
    removal TEXT CHECK (removal IS NULL OR removal IN ('deleted', 'too_old', 'failed')),
    UNIQUE (announcement_id, round, link_id)
);
CREATE INDEX announcement_delivery_send ON announcement_delivery (announcement_id, round, channel, id);
CREATE INDEX announcement_delivery_student ON announcement_delivery (student_id, announcement_id);
