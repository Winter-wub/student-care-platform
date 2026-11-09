-- The "fees" Module (#127 review): who uploaded each import, so its result
-- page follows the uploader rule of the kept upload it replaces (only the
-- uploader sees it); and the result's count of Students per upload.
ALTER TABLE fees_import ADD COLUMN staff_id TEXT;
CREATE INDEX fees_balance_upload ON fees_balance (upload_id);
