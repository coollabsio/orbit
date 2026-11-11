-- Incoming webhooks: a secret URL that posts messages into one channel (apps/server/src/chat_routes/hooks.rs).
CREATE TABLE chat_webhooks (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    -- https only; NULL shows the default bot avatar.
    icon_url TEXT CHECK (icon_url IS NULL OR length(icon_url) <= 2048),
    -- SHA-256 of the secret in the URL. The secret itself is never stored.
    secret_hash BLOB NOT NULL CHECK (length(secret_hash) = 32),
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    rotated_at INTEGER,
    last_used_at INTEGER,
    revoked_at INTEGER
);

CREATE INDEX chat_webhooks_conversation ON chat_webhooks (conversation_id, created_at)
    WHERE revoked_at IS NULL;

CREATE TRIGGER chat_webhooks_scope_insert
BEFORE INSERT ON chat_webhooks
WHEN NOT EXISTS (
    SELECT 1 FROM chat_conversations
    WHERE chat_conversations.id = NEW.conversation_id
      AND chat_conversations.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'chat webhook must belong to the conversation''s workspace');
END;

-- Set on a message that a webhook posted. `author_id` stays the webhook's creator. Not a foreign key:
-- the message keeps its bot name and icon as they were when it was sent.
ALTER TABLE chat_messages ADD COLUMN webhook_id TEXT;
ALTER TABLE chat_messages ADD COLUMN bot_name TEXT;
ALTER TABLE chat_messages ADD COLUMN bot_icon_url TEXT;
-- The bot name of a forwarded webhook message, so the copy names the bot, not its creator.
ALTER TABLE chat_messages ADD COLUMN forward_bot_name TEXT;
