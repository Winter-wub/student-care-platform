-- Issue #5 measurement (f): 500-row upsert batch. Not part of the product.
CREATE TABLE IF NOT EXISTS bench_roster (
    student_code TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    email TEXT,
    run TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
