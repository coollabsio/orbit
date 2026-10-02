-- Chat files. The bytes live in the shared attachment blob store (deduplicated per workspace);
-- a row is one message's reference to a blob. A file is uploaded first and belongs to its
-- uploader only; the send of the message attaches it. Files that are never attached are
-- removed after a day. When a row goes, its blob is quarantined for a day; upload
-- reconciliation then reclaims blobs that nothing references any more.
CREATE TABLE chat_message_files (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    -- Both NULL until the message is sent.
    message_id TEXT REFERENCES chat_messages(id) ON DELETE CASCADE,
    conversation_id TEXT REFERENCES chat_conversations(id) ON DELETE CASCADE,
    blob_id TEXT NOT NULL REFERENCES attachment_blobs(id) ON DELETE RESTRICT,
    file_name TEXT NOT NULL CHECK (length(file_name) BETWEEN 1 AND 255),
    -- Detected from the file's bytes.
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    -- Of an image, as the uploader's browser measured it: only a hint to reserve space.
    width INTEGER CHECK (width IS NULL OR width BETWEEN 1 AND 100000),
    height INTEGER CHECK (height IS NULL OR height BETWEEN 1 AND 100000),
    uploaded_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    CHECK ((message_id IS NULL) = (conversation_id IS NULL))
);

CREATE INDEX chat_message_files_message ON chat_message_files (message_id);
-- The files list of a conversation: its messages that have files, newest first.
CREATE INDEX chat_message_files_conversation ON chat_message_files (conversation_id, message_id);
CREATE INDEX chat_message_files_blob ON chat_message_files (blob_id);
CREATE INDEX chat_message_files_workspace ON chat_message_files (workspace_id);
CREATE INDEX chat_message_files_pending ON chat_message_files (created_at)
    WHERE message_id IS NULL;

CREATE TRIGGER chat_message_files_scope_insert
BEFORE INSERT ON chat_message_files
WHEN NOT EXISTS (
    SELECT 1 FROM attachment_blobs
    WHERE attachment_blobs.id = NEW.blob_id AND attachment_blobs.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'chat file scope mismatch');
END;

CREATE TRIGGER chat_message_files_scope_immutable
BEFORE UPDATE OF workspace_id, blob_id ON chat_message_files
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.blob_id <> OLD.blob_id
BEGIN
    SELECT RAISE(ABORT, 'chat file scope is immutable');
END;

-- Fires for direct deletes and for the message, conversation and workspace cascades alike.
CREATE TRIGGER chat_message_files_quarantine_blob
AFTER DELETE ON chat_message_files
BEGIN
    UPDATE attachment_blobs
    SET quarantine_until = MAX(
        quarantine_until,
        CAST(strftime('%s', 'now') AS INTEGER) * 1000 + 86400000
    )
    WHERE id = OLD.blob_id;
END;
