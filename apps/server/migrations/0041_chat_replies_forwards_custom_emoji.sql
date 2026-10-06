-- Inline replies, forwards, custom emoji and custom stickers for chat. One migration for the four
-- features: they were built together.

-- 1. Inline replies

-- A message that quotes another message of the same conversation (not a thread reply: it stays
-- in the list it was sent to). The server checks the target when the message is sent.
-- No foreign key: a message is hard-deleted, and the reply keeps the id so the client can show
-- that the original message was deleted. The quoted author and text are read with a join on
-- `chat_messages.id` (unique) each time, so an edit of the target shows in the quote.
-- No index: nothing lists the replies to a message.
ALTER TABLE chat_messages ADD COLUMN reply_to_id TEXT;

-- 2. Forwards

-- A forward is a new message in the destination conversation, written by the member who
-- forwards. It is a snapshot: its `body` is a copy of the original's body, and its files are
-- new `chat_message_files` rows that point at the same blobs. An edit or the deletion of the
-- original changes nothing in it.
-- The four columns say where the copy came from; all four are set on a forward and NULL on
-- every other message. A forward of a forward keeps the values of the first original.
-- No foreign keys: the original message (and its conversation) can be hard-deleted, and the
-- forward keeps the ids. Users are never hard-deleted, but the author is read from this row
-- with no join, so it needs no reference.
-- No index: nothing lists the forwards of a message.

-- The original message.
ALTER TABLE chat_messages ADD COLUMN forward_of_id TEXT;
-- The conversation of the original message.
ALTER TABLE chat_messages ADD COLUMN forward_conversation_id TEXT;
-- The author of the original message.
ALTER TABLE chat_messages ADD COLUMN forward_author_id TEXT;
-- When the original message was sent, in milliseconds.
ALTER TABLE chat_messages ADD COLUMN forward_created_at INTEGER;

-- 3. Custom emoji

-- The emoji of a workspace: a small image with a name, written `:name:` in a message body and
-- as a reaction. The images are small (at most 256 KiB), so the bytes live in the database, as
-- profile pictures do. The server accepts PNG, JPEG, WebP and GIF only, checked by magic bytes
-- (apps/server/src/chat_routes/images.rs).
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

-- 4. Custom stickers

-- The stickers of a workspace: a larger image with a name, sent as a message of its own (with or
-- without text). The images are at most 512 KiB, so the bytes live in the database, as the emoji
-- do. The server accepts PNG, JPEG, WebP and GIF only, checked by magic bytes
-- (apps/server/src/chat_routes/images.rs).
-- An image never changes: a sticker is replaced by deleting it and adding a new one, which has a
-- new id and so a new image URL, and browsers can cache an image for good.
-- `name` is 2 to 30 characters without control characters; the server trims it and stores it as
-- typed (a sticker is picked from a grid, not typed). NOCASE makes it unique in its workspace
-- whatever the case of its ASCII letters, and orders the list the same way.
-- Users are never hard-deleted, so the user reference needs no ON DELETE action.
CREATE TABLE custom_stickers (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL COLLATE NOCASE CHECK (length(name) BETWEEN 2 AND 30),
    mime_type TEXT NOT NULL CHECK (mime_type IN ('image/png', 'image/jpeg', 'image/webp', 'image/gif')),
    bytes BLOB NOT NULL CHECK (length(bytes) BETWEEN 1 AND 524288),
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    -- Also the index of the list of a workspace, which is ordered by name.
    UNIQUE (workspace_id, name)
);

-- The sticker of a message; the body may be empty then. The server checks the sticker when the
-- message is sent. No foreign key: a sticker can be deleted, and the message keeps the id so the
-- client can show that the sticker is gone. The name and the image URL are read with a join on
-- `custom_stickers.id` (the primary key) each time.
-- No index: nothing lists the messages of a sticker.
ALTER TABLE chat_messages ADD COLUMN sticker_id TEXT;
