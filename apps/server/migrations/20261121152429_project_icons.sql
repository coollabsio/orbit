-- SQLite. Postgres: postgres/20261121152429_project_icons.sql.
--
-- A project's own icon image (issue #146), shown in place of its colour. The image is an attachment blob of the
-- project's workspace; reconcile deletes a replaced or removed one (BLOB_REFERENCE_COUNT).
CREATE TABLE project_icons (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    mime_type TEXT NOT NULL CHECK (mime_type IN ('image/png', 'image/jpeg', 'image/webp')),
    blob_id TEXT NOT NULL REFERENCES attachment_blobs(id) ON DELETE RESTRICT,
    updated_at INTEGER NOT NULL
);
CREATE INDEX project_icons_blob ON project_icons (blob_id);
