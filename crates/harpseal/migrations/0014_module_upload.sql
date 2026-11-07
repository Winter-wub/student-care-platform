-- Kernel migration 0014 (#74).
--
-- A Module's back-office upload kept until the Staff member confirms or
-- cancels it (`Uploads::keep`): the file is in the FileStore under
-- `object_key`, checked against `sha256` when it is read back. Confirming
-- (`Uploads::confirm`) pins it for the import Job: `confirmed_at` is set
-- and `expires_at` moves a week ahead. The tick deletes the file and the
-- row once `expires_at` (ms) has passed. Transient: a backup leaves these
-- rows and their files out (the column is deliberately not `file_key`).
CREATE TABLE module_upload (
    id TEXT PRIMARY KEY,
    module TEXT NOT NULL,
    staff_id TEXT NOT NULL REFERENCES staff (id),
    object_key TEXT NOT NULL,
    file_name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('csv', 'xlsx')),
    size_bytes INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    confirmed_at INTEGER,
    confirmed_by TEXT REFERENCES staff (id)
);
CREATE INDEX module_upload_expires ON module_upload (expires_at);
