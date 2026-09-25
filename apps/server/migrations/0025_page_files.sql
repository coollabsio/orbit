-- Docs page files: images, files and uploaded covers of a page. The bytes live in the shared
-- attachment blob store (deduplicated per workspace); a row is one page's reference to a blob.
-- Rows go with their page (trash purge, teamspace delete, owner delete) and with the workspace.
-- When a row goes, its blob is quarantined for a day; upload reconciliation then reclaims blobs
-- that neither a task attachment nor a page file references any more.
CREATE TABLE page_files (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    blob_id TEXT NOT NULL REFERENCES attachment_blobs(id) ON DELETE RESTRICT,
    file_name TEXT NOT NULL CHECK (length(file_name) BETWEEN 1 AND 255),
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    uploaded_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX page_files_page ON page_files (page_id, created_at, id);
CREATE INDEX page_files_blob ON page_files (blob_id);
CREATE INDEX page_files_workspace ON page_files (workspace_id);
CREATE INDEX page_files_uploaded_by ON page_files (uploaded_by);

CREATE TRIGGER page_files_scope_insert
BEFORE INSERT ON page_files
WHEN NOT EXISTS (
    SELECT 1 FROM pages WHERE pages.id = NEW.page_id AND pages.workspace_id = NEW.workspace_id
) OR NOT EXISTS (
    SELECT 1 FROM attachment_blobs
    WHERE attachment_blobs.id = NEW.blob_id AND attachment_blobs.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'page file scope mismatch');
END;

CREATE TRIGGER page_files_scope_immutable
BEFORE UPDATE OF workspace_id, page_id, blob_id ON page_files
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.page_id <> OLD.page_id
    OR NEW.blob_id <> OLD.blob_id
BEGIN
    SELECT RAISE(ABORT, 'page file scope is immutable');
END;

-- Fires for direct deletes and for the page/workspace cascades alike.
CREATE TRIGGER page_files_quarantine_blob
AFTER DELETE ON page_files
BEGIN
    UPDATE attachment_blobs
    SET quarantine_until = MAX(
        quarantine_until,
        CAST(strftime('%s', 'now') AS INTEGER) * 1000 + 86400000
    )
    WHERE id = OLD.blob_id;
END;
