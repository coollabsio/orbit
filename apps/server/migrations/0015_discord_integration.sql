CREATE TABLE integration_events (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    external_event_id TEXT NOT NULL,
    payload_hash BLOB NOT NULL CHECK (length(payload_hash) = 32),
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, provider, external_event_id)
);

CREATE INDEX integration_events_task ON integration_events (task_id);
