-- Kernel migration 0005 (#23): Job dedupe keys and progress notes.
-- `unique_key` is NULL for ordinary Jobs; `Jobs::enqueue_unique` and the
-- scheduler (`<kind>@<minute>`) set it, and the insert does nothing when a
-- row with the same key exists (until that row is pruned). `progress` is a
-- free-form note the handler writes through `JobContext::progress`.
ALTER TABLE job ADD COLUMN unique_key TEXT;
ALTER TABLE job ADD COLUMN progress TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS job_unique_key ON job (unique_key);
CREATE INDEX IF NOT EXISTS job_finished ON job (state, finished_at);
