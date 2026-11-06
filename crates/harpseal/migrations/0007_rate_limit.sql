-- Kernel migration 0007 (#28): fixed-window counters for rate limits that
-- must hold across Worker isolates (D1). One row per (key, window); a hit
-- is one UPSERT ... RETURNING n. `window` is the window's number (for the
-- Mini App exchange: the epoch minute). Old windows are deleted by the
-- Kernel's hourly Job. Composite primary key, no id (ADR-0007).
CREATE TABLE IF NOT EXISTS rate_limit (
    key TEXT NOT NULL,
    window INTEGER NOT NULL,
    n INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (key, window)
);
