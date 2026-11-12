-- Public API `Idempotency-Key` (apps/server/src/public_api/idempotency.rs): the first response of
-- a POST that made a task or comment, kept per token for 24 hours, so a retry of the same request
-- gets that response instead of a second task or comment. `status` and `response` are NULL while
-- the first request still runs.
CREATE TABLE api_idempotency_keys (
    token_id TEXT NOT NULL REFERENCES api_tokens(id) ON DELETE CASCADE,
    idempotency_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    status INTEGER,
    response TEXT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (token_id, idempotency_key)
);

CREATE INDEX api_idempotency_keys_created ON api_idempotency_keys (created_at);
