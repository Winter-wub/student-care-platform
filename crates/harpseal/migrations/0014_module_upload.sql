-- Kernel migration 0014 (#74).
--
-- A Module's back-office upload kept until the Staff member confirms or
-- cancels it (`SheetUpload::keep`): the file is in the FileStore under
-- `file_key`, checked against `sha256` when it is read back. The tick
-- deletes the file and the row once `expires_at` (ms) has passed.
CREATE TABLE module_upload (
    id TEXT PRIMARY KEY,
    module TEXT NOT NULL,
    staff_id TEXT NOT NULL REFERENCES staff (id),
    file_key TEXT NOT NULL,
    file_name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('csv', 'xlsx')),
    size_bytes INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX module_upload_expires ON module_upload (expires_at);
