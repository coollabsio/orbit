ALTER TABLE api_tokens ADD COLUMN expires_at INTEGER;

CREATE INDEX api_tokens_active_expiration
    ON api_tokens (expires_at)
    WHERE revoked_at IS NULL AND expires_at IS NOT NULL;
