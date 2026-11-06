-- Kernel migration 0004 (#19 review): why a file a Student sent was not
-- copied into the FileStore ('too_large', or the download / storage error,
-- cut to 200 characters). NULL for text and for files that were copied.
ALTER TABLE inbound_message ADD COLUMN file_error TEXT;
