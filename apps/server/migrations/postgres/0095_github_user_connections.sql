-- GitHub user connections, as in migrations/0095_github_user_connections.sql.
ALTER TABLE github_apps ADD COLUMN client_id TEXT;
-- The origin the app was registered from: its OAuth callback URL lives there.
ALTER TABLE github_apps ADD COLUMN callback_origin TEXT;
ALTER TABLE github_apps ADD COLUMN client_secret_encrypted BYTEA;

CREATE TABLE github_user_connections (
    workspace_id TEXT NOT NULL REFERENCES github_apps(workspace_id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    github_user_id BIGINT NOT NULL,
    login TEXT NOT NULL,
    access_token_encrypted BYTEA NOT NULL,
    access_expires_at BIGINT,
    refresh_token_encrypted BYTEA,
    refresh_expires_at BIGINT,
    broken BIGINT NOT NULL DEFAULT 0 CHECK (broken IN (0, 1)),
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (workspace_id, user_id),
    UNIQUE (workspace_id, github_user_id),
    FOREIGN KEY (workspace_id, user_id) REFERENCES memberships(workspace_id, user_id) ON DELETE CASCADE
);

CREATE TABLE github_user_authorizations (
    state_hash BYTEA PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES github_apps(workspace_id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_verifier_encrypted BYTEA NOT NULL,
    client TEXT NOT NULL CHECK (client IN ('web', 'mobile')),
    return_url TEXT,
    -- After GitHub's callback: its code, and the hash of the one-time token the signed-in member finishes with.
    code_encrypted BYTEA,
    finish_hash BYTEA,
    expires_at BIGINT NOT NULL
);
CREATE INDEX github_user_authorizations_expiry ON github_user_authorizations (expires_at);
CREATE UNIQUE INDEX github_user_authorizations_finish ON github_user_authorizations (finish_hash);
