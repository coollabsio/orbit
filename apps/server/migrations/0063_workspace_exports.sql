-- Full workspace exports (apps/server/src/workspace_export): a `workspace.export` job writes a ZIP of the workspace
-- and stores it under `exports/{workspace_id}/{id}.zip`, on the server's disk or in a bucket (`location`).
-- Retention deletes the file and the row once `expires_at` passes. `workspace_id` has no foreign key on purpose:
-- the row outlives a purged workspace until retention has deleted its file.
CREATE TABLE workspace_exports (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    requested_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'ready', 'failed')),
    location TEXT CHECK (location IN ('local', 'bucket', 'workspace_bucket')),
    size_bytes INTEGER CHECK (size_bytes >= 0),
    created_at INTEGER NOT NULL,
    completed_at INTEGER,
    expires_at INTEGER NOT NULL,
    CHECK (
        (status = 'ready' AND location IS NOT NULL AND size_bytes IS NOT NULL AND completed_at IS NOT NULL)
        OR (status <> 'ready' AND location IS NULL)
    )
);

CREATE INDEX workspace_exports_workspace ON workspace_exports (workspace_id, created_at);
CREATE INDEX workspace_exports_expiry ON workspace_exports (expires_at);
-- One export at a time per workspace.
CREATE UNIQUE INDEX workspace_exports_one_active ON workspace_exports (workspace_id)
    WHERE status IN ('queued', 'running');
