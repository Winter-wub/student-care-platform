-- Kernel migration 0003: sending records, usage, Jobs, audit, settings and
-- roster imports (tickets #7, #8, #9). The `migration` table itself is
-- created by the migration runner before the first file runs.

-- Pruned after [retention] outbound_message_days (default 90).
CREATE TABLE outbound_message (
    id TEXT PRIMARY KEY,
    channel TEXT NOT NULL,
    channel_account_id TEXT NOT NULL REFERENCES channel_account (id),
    push INTEGER NOT NULL CHECK (push IN (0, 1)),
    metering TEXT NOT NULL CHECK (metering IN ('metered', 'free')),
    platform_message_id TEXT,
    job_id TEXT,
    module TEXT,
    kind TEXT,
    sent_at INTEGER NOT NULL,
    error TEXT
);
CREATE INDEX outbound_message_sent ON outbound_message (sent_at);
CREATE INDEX outbound_message_account ON outbound_message (channel_account_id, sent_at);

-- Kept forever. `day` is the calendar day in the Deployment's utc_offset,
-- 'YYYY-MM-DD'. Upserted in the same batch as each send.
CREATE TABLE usage_daily (
    channel TEXT NOT NULL,
    day TEXT NOT NULL,
    metered INTEGER NOT NULL DEFAULT 0,
    free INTEGER NOT NULL DEFAULT 0,
    failed INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (channel, day)
);

CREATE TABLE job (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL, -- 'module.name'
    payload TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(payload)),
    run_at INTEGER NOT NULL,
    state TEXT NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'running', 'done', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 3,
    lock_token TEXT,
    locked_until INTEGER,
    last_error TEXT,
    created_at INTEGER NOT NULL,
    finished_at INTEGER
);
CREATE INDEX job_due ON job (state, run_at);

-- Append-only, kept forever. Secrets are redacted before writing.
CREATE TABLE audit_log (
    id TEXT PRIMARY KEY,
    at INTEGER NOT NULL,
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('staff', 'student', 'system', 'job')),
    actor_id TEXT,
    action TEXT NOT NULL, -- e.g. 'link.create', 'student.update', 'staff.login'
    target_kind TEXT,
    target_id TEXT,
    before TEXT CHECK (before IS NULL OR json_valid(before)),
    after TEXT CHECK (after IS NULL OR json_valid(after)),
    ip TEXT,
    user_agent TEXT,
    request_id TEXT
);
CREATE INDEX audit_log_at ON audit_log (at);
CREATE INDEX audit_log_target ON audit_log (target_kind, target_id);
CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only');
END;
CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only');
END;

-- Only catalog text overrides and the Consent notice version. Everything
-- else lives in deployment.toml. `locale` is '' for keys without a Locale.
CREATE TABLE setting (
    key TEXT NOT NULL,
    locale TEXT NOT NULL DEFAULT '',
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    updated_by TEXT REFERENCES staff (id),
    PRIMARY KEY (key, locale)
);

CREATE TABLE roster_import (
    id TEXT PRIMARY KEY,
    staff_id TEXT NOT NULL REFERENCES staff (id),
    mode TEXT NOT NULL CHECK (mode IN ('full', 'upsert')),
    state TEXT NOT NULL CHECK (state IN (
        'previewed', 'queued', 'running', 'done', 'failed', 'cancelled')),
    file_key TEXT NOT NULL, -- kept in the FileStore for one year
    file_name TEXT NOT NULL,
    total_rows INTEGER NOT NULL DEFAULT 0,
    applied_rows INTEGER NOT NULL DEFAULT 0,
    counts TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(counts)),
    error TEXT,
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    finished_at INTEGER
);
CREATE INDEX roster_import_state ON roster_import (state, created_at);
