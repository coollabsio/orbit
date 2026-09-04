CREATE TABLE installation_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    initialized INTEGER NOT NULL DEFAULT 0 CHECK (initialized IN (0, 1)),
    initialized_at INTEGER
);

CREATE TABLE jobs (
    id TEXT PRIMARY KEY,
    workspace_id TEXT,
    schedule_id TEXT REFERENCES schedules(id) ON DELETE SET NULL,
    scheduled_for INTEGER,
    manual_retry_of TEXT REFERENCES jobs(id) ON DELETE SET NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'dead')),
    priority TEXT NOT NULL DEFAULT 'normal'
        CHECK (priority IN ('low', 'normal', 'high', 'critical')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    max_attempts INTEGER NOT NULL CHECK (max_attempts > 0),
    available_at INTEGER NOT NULL,
    lease_owner TEXT,
    claim_token TEXT,
    lease_expires_at INTEGER,
    last_error TEXT,
    dead_at INTEGER,
    incident_hold INTEGER NOT NULL DEFAULT 0 CHECK (incident_hold IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (schedule_id, scheduled_for)
);

CREATE INDEX jobs_claimable
    ON jobs (state, available_at, lease_expires_at);

CREATE TABLE job_attempts (
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    attempt INTEGER NOT NULL CHECK (attempt > 0),
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    outcome TEXT,
    error TEXT,
    PRIMARY KEY (job_id, attempt)
);

CREATE TABLE schedules (
    id TEXT PRIMARY KEY,
    workspace_id TEXT,
    job_kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    schedule TEXT NOT NULL,
    next_run_at INTEGER NOT NULL,
    last_materialized_at INTEGER,
    catch_up_mode TEXT NOT NULL DEFAULT 'latest'
        CHECK (catch_up_mode IN ('latest', 'full')),
    consecutive_failures INTEGER NOT NULL DEFAULT 0 CHECK (consecutive_failures >= 0),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    updated_at INTEGER NOT NULL
);

CREATE INDEX schedules_due ON schedules (enabled, next_run_at);

CREATE TABLE audit_events (
    id TEXT PRIMARY KEY,
    workspace_id TEXT,
    actor_id TEXT,
    action TEXT NOT NULL,
    outcome TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    request_id TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    occurred_at INTEGER NOT NULL
);

CREATE INDEX audit_events_workspace_time
    ON audit_events (workspace_id, occurred_at);

CREATE TABLE outbox_events (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    workspace_id TEXT,
    topic TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    published_at INTEGER,
    UNIQUE (scope, sequence)
);

CREATE INDEX outbox_events_unpublished
    ON outbox_events (published_at, created_at);

CREATE TABLE attachment_blobs (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    storage_key TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    quarantine_until INTEGER NOT NULL,
    UNIQUE (workspace_id, sha256, byte_size),
    UNIQUE (storage_key)
);

CREATE TABLE pending_uploads (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    temporary_path TEXT NOT NULL,
    original_name TEXT NOT NULL,
    media_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    sha256 TEXT,
    state TEXT NOT NULL CHECK (state IN ('receiving', 'staged', 'complete', 'failed')),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    completed_at INTEGER
);

CREATE INDEX pending_uploads_expiry
    ON pending_uploads (state, expires_at);
