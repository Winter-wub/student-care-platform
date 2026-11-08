-- Announcements Module (#109): files Staff attach to an announcement.
--
-- Images first (kind 'image', slots 1 to 4, the N of `![alt](image:N)` in
-- a body); PDFs (#113) reuse the table with their own kind. The bytes are
-- in the FileStore under `file_key`; backup and restore carry them through
-- that column (ADR-0008: `file_key` columns).
--
-- An edit that removes or replaces a file of an announcement not sent yet
-- (draft, scheduled) deletes its row and its object. Once sent, Cards
-- already delivered point at the file: the row is kept with
-- `superseded_at` set and the file stays served (same token, same live
-- check) until the announcement is retracted or expires. One file is
-- current per slot (`announcement_file_slot`).
--
-- `token` is the public path `/announcements/files/<token>`: 32 hex
-- characters (128 bits from the Random port), served only while the
-- announcement is sent and live. A replaced file gets a new token, so a
-- platform's cached copy of the old one is never shown for the new one.
CREATE TABLE announcement_file (
    token TEXT NOT NULL PRIMARY KEY,
    announcement_id TEXT NOT NULL REFERENCES announcement (id),
    kind TEXT NOT NULL CHECK (kind IN ('image', 'pdf')),
    n INTEGER NOT NULL CHECK (n BETWEEN 1 AND 9),
    content_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL CHECK (size_bytes > 0),
    -- Pixels, when the file is an image whose header gave them.
    width INTEGER,
    height INTEGER,
    -- The name shown (images: generated; PDFs: as uploaded, made safe).
    file_name TEXT NOT NULL,
    file_key TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    -- Replaced or removed after the announcement was sent; NULL: current.
    superseded_at INTEGER
);

CREATE UNIQUE INDEX announcement_file_slot ON announcement_file (announcement_id, kind, n)
    WHERE superseded_at IS NULL;
