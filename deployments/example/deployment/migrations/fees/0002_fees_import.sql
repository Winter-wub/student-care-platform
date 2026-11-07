-- The "fees" Module (#74 review): one row per imported upload, written in
-- the same batch as its balances, so a retried Job that finds it only
-- deletes the kept file and never writes older balances again.
CREATE TABLE fees_import (
    upload_id TEXT PRIMARY KEY,
    rows INTEGER NOT NULL,
    imported_at INTEGER NOT NULL
);
