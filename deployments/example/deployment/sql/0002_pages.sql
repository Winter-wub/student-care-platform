-- Issue #5 prototype: the subset of the #7 / #10 tables the two pages need.
-- Ids are UUIDv7 text, times are INTEGER ms UTC (ADR-0007).
CREATE TABLE student (
    id TEXT PRIMARY KEY,
    student_code TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    email TEXT,
    locale TEXT NOT NULL DEFAULT 'th',
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'left')),
    attributes TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX student_name ON student (display_name);

CREATE TABLE staff (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL, -- PHC string
    status TEXT NOT NULL DEFAULT 'active',
    locale TEXT NOT NULL DEFAULT 'th'
);

CREATE TABLE staff_session (
    id TEXT PRIMARY KEY, -- SHA-256 of the cookie token
    staff_id TEXT NOT NULL REFERENCES staff (id),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    ip TEXT
);

CREATE TABLE announcement (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL, -- draft / scheduled / sending / sent / retracted
    audience TEXT NOT NULL, -- JSON
    require_ack INTEGER NOT NULL DEFAULT 1,
    created_by TEXT,
    created_at INTEGER NOT NULL
);

CREATE TABLE announcement_text (
    announcement_id TEXT NOT NULL REFERENCES announcement (id),
    locale TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY (announcement_id, locale)
);

-- Prototype stand-in for announcement_delivery.acked_at (#10).
CREATE TABLE announcement_ack (
    announcement_id TEXT NOT NULL,
    student_id TEXT NOT NULL,
    acked_at INTEGER NOT NULL,
    PRIMARY KEY (announcement_id, student_id)
);
