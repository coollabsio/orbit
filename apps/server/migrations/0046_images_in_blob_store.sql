-- Profile pictures, custom emoji and stickers move to the attachment blob store, so they go to S3 with all other
-- files when it is on (crates/platform/src/files/tiered.rs).
--
-- New images have a `blob_id` and no `bytes`. Rows from before keep their `bytes` until the server's background
-- mover moves them to the blob store (object_storage.rs); the server reads both. A blob of an emoji or a
-- sticker is in its workspace; a blob of a profile picture uses the user id as its workspace id (blobs are only
-- grouped by that id; it is not a foreign key). Reconcile deletes a blob that nothing references
-- (BLOB_REFERENCE_COUNT), so a replaced or deleted image needs no cleanup here.
--
-- SQLite cannot drop a NOT NULL constraint, so the three tables are rebuilt as in 0021. No table references them
-- (chat_messages.sticker_id is not a foreign key) and they have no triggers or separate indexes.
PRAGMA defer_foreign_keys = ON;

CREATE TABLE user_avatars_backup AS SELECT * FROM user_avatars;
DROP TABLE user_avatars;
CREATE TABLE user_avatars (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    mime_type TEXT NOT NULL CHECK (mime_type IN ('image/png', 'image/jpeg', 'image/webp')),
    bytes BLOB CHECK (length(bytes) BETWEEN 1 AND 524288),
    blob_id TEXT REFERENCES attachment_blobs(id) ON DELETE RESTRICT,
    updated_at INTEGER NOT NULL,
    CHECK ((bytes IS NULL) <> (blob_id IS NULL))
);
INSERT INTO user_avatars (user_id, mime_type, bytes, updated_at)
SELECT user_id, mime_type, bytes, updated_at FROM user_avatars_backup;
DROP TABLE user_avatars_backup;
CREATE INDEX user_avatars_blob ON user_avatars (blob_id);

CREATE TABLE custom_emoji_backup AS SELECT * FROM custom_emoji;
DROP TABLE custom_emoji;
CREATE TABLE custom_emoji (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 2 AND 32 AND name NOT GLOB '*[^a-z0-9_]*'),
    mime_type TEXT NOT NULL CHECK (mime_type IN ('image/png', 'image/jpeg', 'image/webp', 'image/gif')),
    bytes BLOB CHECK (length(bytes) BETWEEN 1 AND 262144),
    blob_id TEXT REFERENCES attachment_blobs(id) ON DELETE RESTRICT,
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    -- Also the index of the list of a workspace, which is ordered by name.
    UNIQUE (workspace_id, name),
    CHECK ((bytes IS NULL) <> (blob_id IS NULL))
);
INSERT INTO custom_emoji (id, workspace_id, name, mime_type, bytes, created_by, created_at)
SELECT id, workspace_id, name, mime_type, bytes, created_by, created_at FROM custom_emoji_backup;
DROP TABLE custom_emoji_backup;
CREATE INDEX custom_emoji_blob ON custom_emoji (blob_id);

CREATE TABLE custom_stickers_backup AS SELECT * FROM custom_stickers;
DROP TABLE custom_stickers;
CREATE TABLE custom_stickers (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL COLLATE NOCASE CHECK (length(name) BETWEEN 2 AND 30),
    mime_type TEXT NOT NULL CHECK (mime_type IN ('image/png', 'image/jpeg', 'image/webp', 'image/gif')),
    bytes BLOB CHECK (length(bytes) BETWEEN 1 AND 524288),
    blob_id TEXT REFERENCES attachment_blobs(id) ON DELETE RESTRICT,
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    -- Also the index of the list of a workspace, which is ordered by name.
    UNIQUE (workspace_id, name),
    CHECK ((bytes IS NULL) <> (blob_id IS NULL))
);
INSERT INTO custom_stickers (id, workspace_id, name, mime_type, bytes, created_by, created_at)
SELECT id, workspace_id, name, mime_type, bytes, created_by, created_at FROM custom_stickers_backup;
DROP TABLE custom_stickers_backup;
CREATE INDEX custom_stickers_blob ON custom_stickers (blob_id);
