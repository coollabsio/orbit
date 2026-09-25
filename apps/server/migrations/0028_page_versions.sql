-- Docs page history: snapshots of a page's title, icon and content. A snapshot of the previous
-- state is taken inside the save transaction when the title or content changes and the newest
-- snapshot is at least 10 minutes old; a version restore snapshots the current state first
-- (kind 'restore'); a Notion import stores the imported content (kind 'import'). Retention
-- (workspace.retention job) keeps everything for 30 days, then one per day for a year, and
-- always the newest 20 of a page. Rows go with their page (trash purge, teamspace delete,
-- workspace purge) and with the workspace. Page files referenced by old versions stay while the
-- page exists.
CREATE TABLE page_versions (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    icon TEXT,
    content_json TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('auto', 'restore', 'import')),
    -- Who last edited the snapshotted state (the importer for 'import').
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX page_versions_page_created ON page_versions (page_id, created_at DESC, id DESC);
CREATE INDEX page_versions_workspace ON page_versions (workspace_id);
CREATE INDEX page_versions_created_by ON page_versions (created_by);

CREATE TRIGGER page_versions_scope_insert
BEFORE INSERT ON page_versions
WHEN NOT EXISTS (
    SELECT 1 FROM pages WHERE pages.id = NEW.page_id AND pages.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'page version must belong to the page workspace');
END;

CREATE TRIGGER page_versions_scope_immutable
BEFORE UPDATE OF workspace_id, page_id ON page_versions
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.page_id <> OLD.page_id
BEGIN
    SELECT RAISE(ABORT, 'page version scope is immutable');
END;
