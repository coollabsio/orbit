-- As in apps/server/migrations/20261112082908_api_idempotency_keys.sql.
CREATE TABLE api_idempotency_keys (
    token_id TEXT NOT NULL REFERENCES api_tokens(id) ON DELETE CASCADE,
    idempotency_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    status BIGINT,
    response TEXT,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (token_id, idempotency_key)
);

CREATE INDEX api_idempotency_keys_created ON api_idempotency_keys (created_at);
