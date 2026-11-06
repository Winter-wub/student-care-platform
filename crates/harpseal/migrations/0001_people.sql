-- Kernel migration 0001: Students, Tags and Staff access (ticket #7, ADR-0007).
-- Ids are UUIDv7 text, times are INTEGER milliseconds UTC, JSON is TEXT.
-- SQL must run on both SQLite (rusqlite) and Cloudflare D1.

CREATE TABLE student (
    id TEXT PRIMARY KEY,
    student_code TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    email TEXT,
    locale TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'left')),
    -- Values for the attribute schema declared in deployment.toml.
    attributes TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(attributes)),
    -- JSON array of field names Staff edited by hand; imports skip them.
    manual_overrides TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(manual_overrides)),
    verification_locked_until INTEGER,
    roster_imported_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX student_display_name ON student (display_name);
CREATE INDEX student_status ON student (status);

CREATE TABLE staff (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE, -- stored lowercased
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL, -- PHC string, Argon2id
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled')),
    locale TEXT NOT NULL,
    must_change_password INTEGER NOT NULL DEFAULT 0 CHECK (must_change_password IN (0, 1)),
    failed_logins INTEGER NOT NULL DEFAULT 0,
    locked_until INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE tag (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);

-- A roster Tag and a manual Tag of the same name may coexist; an import
-- replaces only the 'roster' rows.
CREATE TABLE student_tag (
    student_id TEXT NOT NULL REFERENCES student (id),
    tag_id TEXT NOT NULL REFERENCES tag (id),
    source TEXT NOT NULL CHECK (source IN ('roster', 'manual')),
    added_at INTEGER NOT NULL,
    added_by TEXT REFERENCES staff (id), -- manual Tags; NULL for roster Tags
    PRIMARY KEY (student_id, tag_id, source)
);
CREATE INDEX student_tag_tag ON student_tag (tag_id);

CREATE TABLE role (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);

-- Permission ids are declared in code by Modules; there is no table for them.
CREATE TABLE role_permission (
    role_id TEXT NOT NULL REFERENCES role (id) ON DELETE CASCADE,
    permission_id TEXT NOT NULL,
    PRIMARY KEY (role_id, permission_id)
);

CREATE TABLE staff_role (
    staff_id TEXT NOT NULL REFERENCES staff (id) ON DELETE CASCADE,
    role_id TEXT NOT NULL REFERENCES role (id) ON DELETE CASCADE,
    PRIMARY KEY (staff_id, role_id)
);
CREATE INDEX staff_role_role ON staff_role (role_id);

CREATE TABLE staff_session (
    id TEXT PRIMARY KEY, -- SHA-256 hex of the cookie token, never the token
    staff_id TEXT NOT NULL REFERENCES staff (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL, -- absolute limit
    last_seen_at INTEGER NOT NULL, -- sliding limit is computed from it
    ip TEXT,
    user_agent TEXT
);
CREATE INDEX staff_session_staff ON staff_session (staff_id);
