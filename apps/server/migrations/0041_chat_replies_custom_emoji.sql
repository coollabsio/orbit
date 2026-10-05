-- Inline replies and custom emoji for chat. One migration for the two features: they were
-- built together.

-- 1. Inline replies

-- A message that quotes another message of the same conversation (not a thread reply: it stays
-- in the list it was sent to). The server checks the target when the message is sent.
-- No foreign key: a message is hard-deleted, and the reply keeps the id so the client can show
-- that the original message was deleted. The quoted author and text are read with a join on
-- `chat_messages.id` (unique) each time, so an edit of the target shows in the quote.
-- No index: nothing lists the replies to a message.
ALTER TABLE chat_messages ADD COLUMN reply_to_id TEXT;

-- 2. Custom emoji

-- The emoji of a workspace: a small image with a name, written `:name:` in a message body and
-- as a reaction. The images are small (at most 256 KiB), so the bytes live in the database, as
-- profile pictures do. The server accepts PNG, JPEG, WebP and GIF only, checked by magic bytes
-- (apps/server/src/chat_routes/emoji.rs).
-- An image never changes: an emoji is replaced by deleting it and adding a new one, which has a
-- new id and so a new image URL, and browsers can cache an image for good.
-- `name` is 2 to 32 characters of `a-z 0-9 _`, unique in its workspace; the server lower-cases it.
-- Users are never hard-deleted, so the user reference needs no ON DELETE action.
CREATE TABLE custom_emoji (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 2 AND 32 AND name NOT GLOB '*[^a-z0-9_]*'),
    mime_type TEXT NOT NULL CHECK (mime_type IN ('image/png', 'image/jpeg', 'image/webp', 'image/gif')),
    bytes BLOB NOT NULL CHECK (length(bytes) BETWEEN 1 AND 262144),
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    -- Also the index of the list of a workspace, which is ordered by name.
    UNIQUE (workspace_id, name)
);
