-- Interactive buttons (apps/server/src/chat_routes/interactions.rs). A click is sent, signed, to the webhook's
-- request URL; the integration answers through a short-lived response URL.
ALTER TABLE chat_webhooks ADD COLUMN request_url TEXT CHECK (request_url IS NULL OR length(request_url) <= 2048);
-- secret_box ciphertext of the signing secret. Orbit signs with it, so it is encrypted, not hashed.
ALTER TABLE chat_webhooks ADD COLUMN signing_secret BLOB;

CREATE TABLE chat_action_responses (
    token_hash BLOB PRIMARY KEY CHECK (length(token_hash) = 32),
    webhook_id TEXT NOT NULL REFERENCES chat_webhooks(id) ON DELETE CASCADE,
    -- Not a foreign key: a response may still post after the original message is deleted.
    message_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
    -- Who clicked: the receiver of an ephemeral reply.
    user_id TEXT NOT NULL REFERENCES users(id),
    uses_left INTEGER NOT NULL CHECK (uses_left >= 0),
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX chat_action_responses_expiry ON chat_action_responses (expires_at);

-- The newest deliveries of each webhook, for the Integrations tab.
CREATE TABLE chat_webhook_deliveries (
    id TEXT PRIMARY KEY,
    webhook_id TEXT NOT NULL REFERENCES chat_webhooks(id) ON DELETE CASCADE,
    action_id TEXT NOT NULL,
    status INTEGER,
    error TEXT,
    duration_ms INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX chat_webhook_deliveries_webhook ON chat_webhook_deliveries (webhook_id, created_at);
