-- A workspace's own S3 bucket for its attachment files (Settings → File storage). `active = 0`: the workspace
-- removed the bucket and its files move back to the installation's storage; the row goes when none are left.
CREATE TABLE workspace_storage (
    workspace_id TEXT PRIMARY KEY NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    endpoint TEXT NOT NULL,
    region TEXT NOT NULL,
    bucket TEXT NOT NULL,
    prefix TEXT NOT NULL DEFAULT '',
    access_key_id TEXT NOT NULL,
    secret_access_key BLOB NOT NULL,
    path_style INTEGER NOT NULL DEFAULT 0 CHECK (path_style IN (0, 1)),
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    updated_at INTEGER NOT NULL
);
