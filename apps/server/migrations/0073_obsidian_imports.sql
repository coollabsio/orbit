-- Obsidian imports: one row per vault (zip) a member uploaded to import into Docs. The upload is
-- stored like a workspace export (`archive_location`: server disk, installation bucket or the
-- workspace's own bucket, key `imports/{workspace_id}/{id}.zip`) until the import reaches a final
-- state; the archive is then deleted and `archive_location` cleared (the cleanup retries a
-- deletion that failed). An import belongs to its creator: it goes with the user and with the
-- workspace. Its destination is recorded at start; deleting the destination teamspace or parent
-- page clears the reference and the running import then fails instead of guessing a new place.
CREATE TABLE obsidian_imports (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN (
        'scanning', 'ready', 'queued', 'importing', 'completed', 'failed', 'cancelled', 'expired'
    )),
    file_name TEXT NOT NULL DEFAULT '',
    archive_bytes INTEGER NOT NULL DEFAULT 0 CHECK (archive_bytes >= 0),
    archive_location TEXT CHECK (archive_location IN ('local', 'bucket', 'workspace_bucket')),
    tree_json TEXT,
    selection_json TEXT,
    target_teamspace_id TEXT REFERENCES teamspaces(id) ON DELETE SET NULL,
    target_private INTEGER NOT NULL DEFAULT 0 CHECK (target_private IN (0, 1)),
    target_parent_page_id TEXT REFERENCES pages(id) ON DELETE SET NULL,
    total INTEGER NOT NULL DEFAULT 0 CHECK (total >= 0),
    done INTEGER NOT NULL DEFAULT 0 CHECK (done >= 0),
    failed INTEGER NOT NULL DEFAULT 0 CHECK (failed >= 0),
    report_json TEXT,
    error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (target_private = 0 OR target_teamspace_id IS NULL)
);

CREATE INDEX obsidian_imports_user ON obsidian_imports (user_id, workspace_id, created_at);
CREATE INDEX obsidian_imports_workspace ON obsidian_imports (workspace_id);
CREATE INDEX obsidian_imports_status ON obsidian_imports (status, updated_at);
CREATE INDEX obsidian_imports_target_teamspace ON obsidian_imports (target_teamspace_id);
CREATE INDEX obsidian_imports_target_parent ON obsidian_imports (target_parent_page_id);

-- The destination must belong to the import's workspace.
CREATE TRIGGER obsidian_imports_target_scope_insert
BEFORE INSERT ON obsidian_imports
WHEN (NEW.target_teamspace_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM teamspaces
    WHERE teamspaces.id = NEW.target_teamspace_id AND teamspaces.workspace_id = NEW.workspace_id
)) OR (NEW.target_parent_page_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM pages
    WHERE pages.id = NEW.target_parent_page_id AND pages.workspace_id = NEW.workspace_id
))
BEGIN
    SELECT RAISE(ABORT, 'obsidian import target scope mismatch');
END;

CREATE TRIGGER obsidian_imports_target_scope_update
BEFORE UPDATE OF workspace_id, target_teamspace_id, target_parent_page_id ON obsidian_imports
WHEN NEW.workspace_id <> OLD.workspace_id
    OR (NEW.target_teamspace_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM teamspaces
        WHERE teamspaces.id = NEW.target_teamspace_id AND teamspaces.workspace_id = NEW.workspace_id
    ))
    OR (NEW.target_parent_page_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM pages
        WHERE pages.id = NEW.target_parent_page_id AND pages.workspace_id = NEW.workspace_id
    ))
BEGIN
    SELECT RAISE(ABORT, 'obsidian import target scope mismatch');
END;

-- The import plan: one row per selected folder or note, in creation order (`position`: parents
-- before children). `path` is the vault path (without the stripped top-level folder) and
-- identifies the item; `note_path` is the markdown file with the page's content (the note, or a
-- folder's folder note). `page_id` is the Orbit page id chosen before the page is created, so a
-- restarted import finds pages it already made instead of creating them twice; it is a record,
-- not a reference (the page may be deleted later).
-- `status`: pending -> created (page exists, no content yet) -> done | failed.
CREATE TABLE obsidian_import_items (
    import_id TEXT NOT NULL REFERENCES obsidian_imports(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('folder', 'note')),
    note_path TEXT,
    parent_path TEXT,
    position INTEGER NOT NULL CHECK (position >= 0),
    title TEXT NOT NULL DEFAULT '',
    page_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'created', 'done', 'failed')),
    error TEXT,
    PRIMARY KEY (import_id, path),
    UNIQUE (import_id, position)
);

CREATE INDEX obsidian_import_items_page ON obsidian_import_items (page_id);
