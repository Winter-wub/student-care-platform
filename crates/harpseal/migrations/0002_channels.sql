-- Kernel migration 0002: Channel Accounts, Links, Verification, Consent,
-- Security Events, Inbound Messages and App Sessions (tickets #7, #9).

CREATE TABLE channel_account (
    id TEXT PRIMARY KEY,
    channel TEXT NOT NULL, -- ChannelKind, e.g. 'line'
    external_id TEXT NOT NULL, -- the platform's user / chat id
    first_seen_at INTEGER NOT NULL,
    platform_locale TEXT,
    blocked_at INTEGER,
    flow_state TEXT NOT NULL DEFAULT 'new' CHECK (flow_state IN (
        'new', 'awaiting_consent', 'awaiting_student_id', 'awaiting_otp', 'linked',
        'awaiting_reconsent')),
    flow_data TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(flow_data)),
    consent_pending_version TEXT, -- notice version shown, accepted on Link
    updated_at INTEGER NOT NULL,
    UNIQUE (channel, external_id)
);

-- History is kept: a revoked Link stays as a row.
CREATE TABLE link (
    id TEXT PRIMARY KEY,
    student_id TEXT NOT NULL REFERENCES student (id),
    channel_account_id TEXT NOT NULL REFERENCES channel_account (id),
    channel TEXT NOT NULL, -- copy of channel_account.channel for the index below
    status TEXT NOT NULL CHECK (status IN ('active', 'unreachable', 'revoked')),
    method TEXT NOT NULL CHECK (method IN ('email_otp', 'manual')),
    verified_by_staff_id TEXT REFERENCES staff (id), -- Manual Links
    linked_at INTEGER NOT NULL,
    revoked_at INTEGER,
    revoked_by TEXT REFERENCES staff (id), -- NULL when the Student or the system revoked
    updated_at INTEGER NOT NULL
);
-- At most one live Link per Student per Channel, and per Channel Account.
-- Deliberately counts 'unreachable' as live: a blocked Link recovers by
-- itself, so it still holds its place.
CREATE UNIQUE INDEX link_live_student_channel ON link (student_id, channel)
    WHERE status <> 'revoked';
CREATE UNIQUE INDEX link_live_account ON link (channel_account_id)
    WHERE status <> 'revoked';

CREATE TABLE verification_challenge (
    id TEXT PRIMARY KEY,
    channel_account_id TEXT NOT NULL REFERENCES channel_account (id),
    student_id TEXT NOT NULL REFERENCES student (id),
    method TEXT NOT NULL, -- VerificationMethod::id
    secret_hash TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN (
        'pending', 'passed', 'failed', 'expired', 'cancelled')),
    created_at INTEGER NOT NULL
);
-- Resend limit = challenges for the Student in the last hour.
CREATE INDEX verification_challenge_student ON verification_challenge (student_id, created_at);
CREATE INDEX verification_challenge_account ON verification_challenge (channel_account_id, state);

-- One row per Student per accepted notice version. Never changed or deleted.
CREATE TABLE consent (
    id TEXT PRIMARY KEY,
    student_id TEXT NOT NULL REFERENCES student (id),
    channel_account_id TEXT NOT NULL REFERENCES channel_account (id),
    notice_version TEXT NOT NULL,
    locale TEXT NOT NULL,
    accepted_at INTEGER NOT NULL,
    UNIQUE (student_id, notice_version)
);
CREATE INDEX consent_student ON consent (student_id, accepted_at);
CREATE TRIGGER consent_no_update BEFORE UPDATE ON consent
BEGIN
    SELECT RAISE(ABORT, 'consent rows are never changed');
END;
CREATE TRIGGER consent_no_delete BEFORE DELETE ON consent
BEGIN
    SELECT RAISE(ABORT, 'consent rows are never deleted');
END;

-- Append-only.
CREATE TABLE security_event (
    id TEXT PRIMARY KEY,
    at INTEGER NOT NULL,
    kind TEXT NOT NULL, -- e.g. 'otp.lockout', 'link.duplicate_attempt', 'staff.lockout'
    student_id TEXT REFERENCES student (id),
    channel_account_id TEXT REFERENCES channel_account (id),
    staff_id TEXT REFERENCES staff (id),
    details TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(details))
);
CREATE INDEX security_event_at ON security_event (at);
CREATE TRIGGER security_event_no_update BEFORE UPDATE ON security_event
BEGIN
    SELECT RAISE(ABORT, 'security_event is append-only');
END;
CREATE TRIGGER security_event_no_delete BEFORE DELETE ON security_event
BEGIN
    SELECT RAISE(ABORT, 'security_event is append-only');
END;

-- Free text and files a linked Student sent outside any Action.
CREATE TABLE inbound_message (
    id TEXT PRIMARY KEY,
    channel_account_id TEXT NOT NULL REFERENCES channel_account (id),
    student_id TEXT REFERENCES student (id),
    kind TEXT NOT NULL CHECK (kind IN ('text', 'file')),
    text TEXT,
    file_key TEXT, -- FileStore key of the copied file
    file_name TEXT,
    content_type TEXT,
    size_bytes INTEGER,
    received_at INTEGER NOT NULL
);
CREATE INDEX inbound_message_account ON inbound_message (channel_account_id, received_at);

CREATE TABLE app_session (
    id TEXT PRIMARY KEY, -- SHA-256 hex of the cookie token
    channel_account_id TEXT NOT NULL REFERENCES channel_account (id),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX app_session_account ON app_session (channel_account_id);
