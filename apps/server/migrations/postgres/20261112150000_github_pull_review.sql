-- GitHub pull request review, as in migrations/20261112150000_github_pull_review.sql: user connections, then notices.
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

-- Pull request notices.
ALTER TABLE notification_prefs ADD COLUMN github BIGINT NOT NULL DEFAULT 1 CHECK (github IN (0, 1));

ALTER TABLE notifications ADD COLUMN github_repository TEXT;
ALTER TABLE notifications ADD COLUMN github_pull_number BIGINT;
ALTER TABLE notifications ADD COLUMN github_actor_login TEXT;
ALTER TABLE notifications ADD COLUMN github_review_state TEXT
    CHECK (github_review_state IN ('approved', 'changes_requested', 'commented'));
ALTER TABLE notifications ADD COLUMN github_comment_id BIGINT;

ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications DROP CONSTRAINT notifications_scope_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('task_assigned', 'comment_mentioned', 'description_mentioned',
                    'page_comment_mentioned', 'page_mentioned', 'chat_mentioned',
                    'import_finished', 'github_review_requested', 'github_review',
                    'github_merged'));
ALTER TABLE notifications ADD CONSTRAINT notifications_scope_check
    CHECK (
        (kind IN ('task_assigned', 'comment_mentioned', 'description_mentioned')
            AND task_id IS NOT NULL
            AND page_id IS NULL AND page_thread_id IS NULL AND page_comment_id IS NULL
            AND page_block_id IS NULL)
        OR (kind = 'page_comment_mentioned'
            AND task_id IS NULL AND comment_id IS NULL
            AND page_id IS NOT NULL AND page_thread_id IS NOT NULL AND page_comment_id IS NOT NULL
            AND page_block_id IS NULL)
        OR (kind = 'page_mentioned'
            AND task_id IS NULL AND comment_id IS NULL
            AND page_id IS NOT NULL AND page_thread_id IS NULL AND page_comment_id IS NULL)
        OR (kind IN ('chat_mentioned', 'import_finished', 'github_review_requested', 'github_review', 'github_merged')
            AND task_id IS NULL AND comment_id IS NULL
            AND page_id IS NULL AND page_thread_id IS NULL AND page_comment_id IS NULL
            AND page_block_id IS NULL)
    );
ALTER TABLE notifications ADD CONSTRAINT notifications_github_check
    CHECK (
        ((kind LIKE 'github_%') = (github_repository IS NOT NULL))
        AND ((github_repository IS NULL) = (github_pull_number IS NULL))
        AND ((github_repository IS NULL) = (github_actor_login IS NULL))
        AND ((kind = 'github_review') = (github_review_state IS NOT NULL))
    );
