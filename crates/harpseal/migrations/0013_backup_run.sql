-- Kernel migration 0013 (#78).
--
-- `backup_run`: one row per backup the Job `kernel.backup.run` writes to
-- the FileStore under `prefix` (`backups/<UTC time>/`, backup format 1;
-- ADR-0008). The FileStore port cannot list keys, so this table is the
-- index the Admin page lists and retention prunes from. `state`:
-- `running` while the Job dumps it over one or more runs, `done` once
-- `manifest.json` is written, `failed` when a run gave up (`error` says
-- why). `progress` counts the Job runs so far; the resume point itself
-- is a FileStore object next to the backup (`backups/<UTC time>.progress.json`).
-- `origin`: `schedule` (cron) or `manual` (an Administrator's "back up
-- now", by `requested_by`). A row is deleted with its objects by
-- retention.
CREATE TABLE backup_run (
    id TEXT PRIMARY KEY,
    prefix TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK (state IN ('running', 'done', 'failed')),
    origin TEXT NOT NULL CHECK (origin IN ('schedule', 'manual')),
    requested_by TEXT,
    started_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    finished_at INTEGER,
    progress INTEGER NOT NULL DEFAULT 0,
    bytes INTEGER NOT NULL DEFAULT 0,
    table_count INTEGER,
    row_count INTEGER,
    file_count INTEGER,
    manifest_sha256 TEXT,
    error TEXT
);
CREATE INDEX backup_run_started ON backup_run (started_at);
