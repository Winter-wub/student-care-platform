-- Kernel migration 0008 (#52): the hourly Job `kernel.app.prune_sessions`
-- selects `app_session` rows by `expires_at`; without this index each run
-- scans the table.
CREATE INDEX IF NOT EXISTS app_session_expires_idx ON app_session (expires_at);
