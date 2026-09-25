CREATE TABLE pages (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    parent_id TEXT REFERENCES pages(id) ON DELETE SET NULL,
    title TEXT NOT NULL DEFAULT '',
    icon TEXT,
    cover_url TEXT,
    cover_position TEXT,
    content_json TEXT NOT NULL DEFAULT '[]',
    content_text TEXT NOT NULL DEFAULT '',
    position INTEGER NOT NULL DEFAULT 0,
    creator_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    updated_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    deleted_at INTEGER,
    -- The page whose DELETE trashed this row; restore brings back exactly that batch.
    trashed_with TEXT
);

CREATE INDEX pages_workspace_parent_position ON pages (workspace_id, parent_id, position, id);
CREATE INDEX pages_workspace_deleted ON pages (workspace_id, deleted_at, id);

CREATE TRIGGER pages_workspace_immutable
BEFORE UPDATE OF workspace_id ON pages
WHEN NEW.workspace_id <> OLD.workspace_id
BEGIN
    SELECT RAISE(ABORT, 'page workspace is immutable');
END;

CREATE TRIGGER pages_validate_scope_insert
BEFORE INSERT ON pages
WHEN NEW.parent_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM pages AS parent
    WHERE parent.id = NEW.parent_id AND parent.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'page parent must belong to the workspace');
END;

CREATE TRIGGER pages_validate_scope_update
BEFORE UPDATE OF workspace_id, parent_id ON pages
WHEN NEW.parent_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM pages AS parent
    WHERE parent.id = NEW.parent_id AND parent.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'page parent must belong to the workspace');
END;
