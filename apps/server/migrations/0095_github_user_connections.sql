-- GitHub user access tokens for PR review: the app's OAuth client, one connection for each member, and the
-- single-use states of the OAuth web flow. Apps registered before this have no client and must be re-created.
ALTER TABLE github_apps ADD COLUMN client_id TEXT;
-- The origin the app was registered from: its OAuth callback URL lives there.
ALTER TABLE github_apps ADD COLUMN callback_origin TEXT;
ALTER TABLE github_apps ADD COLUMN client_secret_encrypted BLOB;

CREATE TABLE github_user_connections (
    workspace_id TEXT NOT NULL REFERENCES github_apps(workspace_id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    github_user_id INTEGER NOT NULL,
    login TEXT NOT NULL,
    access_token_encrypted BLOB NOT NULL,
    access_expires_at INTEGER,
    refresh_token_encrypted BLOB,
    refresh_expires_at INTEGER,
    broken INTEGER NOT NULL DEFAULT 0 CHECK (broken IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, user_id),
    UNIQUE (workspace_id, github_user_id),
    FOREIGN KEY (workspace_id, user_id) REFERENCES memberships(workspace_id, user_id) ON DELETE CASCADE
);

CREATE TABLE github_user_authorizations (
    state_hash BLOB PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES github_apps(workspace_id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_verifier_encrypted BLOB NOT NULL,
    client TEXT NOT NULL CHECK (client IN ('web', 'mobile')),
    return_url TEXT,
    expires_at INTEGER NOT NULL
);
CREATE INDEX github_user_authorizations_expiry ON github_user_authorizations (expires_at);
