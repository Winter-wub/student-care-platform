-- Announcements Module (#77): the edit notice, and a note on expiry.
--
-- On a Channel that cannot edit a sent message (LINE), Staff may notify
-- the recipients of an edit: one short Push per recipient linking to the
-- updated announcement, Job `announcements.notice`.
--
-- Since #77 `announcement.expires_at` also closes the announcement to
-- Students: past it, a deep link shows "no longer available" and an ack
-- is refused (0001's comment "acks still accepted" no longer holds).

-- The last notice run: {"version", "at", "sent", "failed", "total"};
-- "total" is NULL until the run's first Job counted the recipients. A
-- save with the notice wanted overwrites it, which stops an older run.
ALTER TABLE announcement ADD COLUMN notice_report TEXT
    CHECK (notice_report IS NULL OR json_valid(notice_report));

-- The announcement version this delivery's recipient was last notified
-- of (or tried and failed); NULL: never. Set once per version, so a
-- re-run notifies no one twice.
ALTER TABLE announcement_delivery ADD COLUMN notice_version INTEGER;
