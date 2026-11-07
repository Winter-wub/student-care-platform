-- The "fees" Module (#74): each Student's outstanding balance, as the last
-- confirmed upload gave it, in satang.
CREATE TABLE fees_balance (
    student_code TEXT PRIMARY KEY,
    amount_satang INTEGER NOT NULL CHECK (amount_satang >= 0),
    upload_id TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
