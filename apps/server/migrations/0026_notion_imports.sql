-- Notion imports: one row per import a member started with a Notion token. The token is kept
-- only as ChaCha20-Poly1305 ciphertext under the app key while the import can still use it; a
-- finished, failed, cancelled or expired import has no token (enforced by a CHECK).
-- An import belongs to its creator: it goes with the user and with the workspace. Its
-- destination is recorded at start; deleting the destination teamspace or parent page clears
-- the reference and the running import then fails instead of guessing a new place.
CREATE TABLE notion_imports (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN (
        'scanning', 'ready', 'queued', 'importing', 'completed', 'failed', 'cancelled', 'expired'
    )),
    token_ciphertext BLOB,
    notion_workspace_name TEXT,
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
    CHECK (token_ciphertext IS NULL OR status IN ('scanning', 'ready', 'queued', 'importing')),
    CHECK (target_private = 0 OR target_teamspace_id IS NULL)
);

CREATE INDEX notion_imports_user ON notion_imports (user_id, workspace_id, created_at);
CREATE INDEX notion_imports_workspace ON notion_imports (workspace_id);
CREATE INDEX notion_imports_status ON notion_imports (status, updated_at);
CREATE INDEX notion_imports_target_teamspace ON notion_imports (target_teamspace_id);
CREATE INDEX notion_imports_target_parent ON notion_imports (target_parent_page_id);

-- The destination must belong to the import's workspace.
CREATE TRIGGER notion_imports_target_scope_insert
BEFORE INSERT ON notion_imports
WHEN (NEW.target_teamspace_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM teamspaces
    WHERE teamspaces.id = NEW.target_teamspace_id AND teamspaces.workspace_id = NEW.workspace_id
)) OR (NEW.target_parent_page_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM pages
    WHERE pages.id = NEW.target_parent_page_id AND pages.workspace_id = NEW.workspace_id
))
BEGIN
    SELECT RAISE(ABORT, 'notion import target scope mismatch');
END;

CREATE TRIGGER notion_imports_target_scope_update
BEFORE UPDATE OF workspace_id, target_teamspace_id, target_parent_page_id ON notion_imports
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
    SELECT RAISE(ABORT, 'notion import target scope mismatch');
END;

-- The import plan: one row per selected Notion page, database or database row, in creation
-- order (`position`: parents before children). `page_id` is the Orbit page id chosen before
-- the page is created, so a restarted import finds pages it already made instead of creating
-- them twice; it is a record, not a reference (the page may be deleted later).
-- `status`: pending -> created (page exists, no content yet) -> done | failed.
CREATE TABLE notion_import_items (
    import_id TEXT NOT NULL REFERENCES notion_imports(id) ON DELETE CASCADE,
    notion_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('page', 'database', 'row')),
    parent_notion_id TEXT,
    position INTEGER NOT NULL CHECK (position >= 0),
    title TEXT NOT NULL DEFAULT '',
    icon TEXT,
    page_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'created', 'done', 'failed')),
    error TEXT,
    PRIMARY KEY (import_id, notion_id),
    UNIQUE (import_id, position)
);

CREATE INDEX notion_import_items_page ON notion_import_items (page_id);
CREATE INDEX notion_import_items_notion ON notion_import_items (notion_id, status);
