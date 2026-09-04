ALTER TABLE users ADD COLUMN email_verified_at INTEGER;

CREATE UNIQUE INDEX memberships_one_owner
    ON memberships (workspace_id)
    WHERE role = 'owner';

CREATE TABLE workspace_invitations (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    email TEXT NOT NULL,
    normalized_email TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member')),
    delivery TEXT NOT NULL CHECK (delivery IN ('manual', 'smtp')),
    token_hash BLOB NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    invited_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    expires_at INTEGER NOT NULL,
    accepted_at INTEGER,
    revoked_at INTEGER,
    replaced_at INTEGER,
    created_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX workspace_invitations_pending_email
    ON workspace_invitations (workspace_id, normalized_email)
    WHERE accepted_at IS NULL AND revoked_at IS NULL AND replaced_at IS NULL;

CREATE INDEX workspace_invitations_workspace_created
    ON workspace_invitations (workspace_id, created_at, id);
