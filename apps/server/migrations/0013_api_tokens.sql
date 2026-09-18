CREATE TABLE api_tokens (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token_hash BLOB NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    token_prefix TEXT NOT NULL,
    can_read INTEGER NOT NULL CHECK (can_read IN (0, 1)),
    can_write INTEGER NOT NULL CHECK (can_write IN (0, 1)),
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER,
    revoked_at INTEGER
);

CREATE INDEX api_tokens_workspace_created ON api_tokens (workspace_id, created_at DESC);
