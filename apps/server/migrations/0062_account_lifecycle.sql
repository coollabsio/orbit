-- Self-serve account deletion and email change (apps/server/src/repositories/account.rs).
--
-- A deleted account keeps its row so tasks, comments and messages keep their author; the row is anonymized
-- ("Deleted user", a placeholder email) and suspended. `deleted_at` marks it so it can never be reinstated.
ALTER TABLE users ADD COLUMN deleted_at INTEGER;

-- A requested email change waits for the one-use link sent to the new address. One live request per account.
CREATE TABLE email_change_tokens (
    token_hash BLOB PRIMARY KEY CHECK (length(token_hash) = 32),
    user_id TEXT NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    email TEXT NOT NULL,
    normalized_email TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX email_change_tokens_expiry ON email_change_tokens (expires_at);
