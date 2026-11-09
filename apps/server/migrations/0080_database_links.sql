-- Docs databases: inline database blocks and linked views. A linked view block shows an existing
-- database with views of its own: a `database_links` row per block, and `database_views.link_id`
-- names the link a view belongs to (NULL: one of the database's own views). The host page's
-- permanent delete removes its links and their views (cascade); a trashed host keeps them.
--
-- The view type check also takes every planned layout, so new layouts need no table rebuild;
-- the server still accepts only the built ones.
CREATE TABLE database_links (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    host_page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX database_links_database ON database_links (database_id);
CREATE INDEX database_links_host ON database_links (host_page_id);
CREATE INDEX database_links_workspace ON database_links (workspace_id);
CREATE INDEX database_links_created_by ON database_links (created_by);

-- A link shows a database of its workspace, in a page of the same workspace.
CREATE TRIGGER database_links_scope_insert
BEFORE INSERT ON database_links
WHEN NOT EXISTS (
    SELECT 1 FROM pages
    WHERE pages.id = NEW.database_id AND pages.workspace_id = NEW.workspace_id AND pages.kind = 'database'
) OR NOT EXISTS (
    SELECT 1 FROM pages WHERE pages.id = NEW.host_page_id AND pages.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'database link scope mismatch');
END;

CREATE TRIGGER database_links_scope_immutable
BEFORE UPDATE OF workspace_id, database_id, host_page_id ON database_links
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.database_id <> OLD.database_id
    OR NEW.host_page_id <> OLD.host_page_id
BEGIN
    SELECT RAISE(ABORT, 'database link scope is immutable');
END;

-- SQLite cannot alter a CHECK constraint, so database_views is rebuilt as in 0079 (see
-- .ai/lessons.md): rows are copied aside, the table is dropped and recreated under the same
-- name, and the rows come back under defer_foreign_keys. No table references database_views.
PRAGMA defer_foreign_keys = ON;

CREATE TABLE database_views_backup AS SELECT * FROM database_views;
DROP TABLE database_views;

CREATE TABLE database_views (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    type TEXT NOT NULL CHECK (type IN ('table', 'board', 'list', 'calendar', 'gallery', 'timeline', 'form')),
    config_json TEXT NOT NULL DEFAULT '{}',
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    link_id TEXT REFERENCES database_links(id) ON DELETE CASCADE
);

INSERT INTO database_views (id, workspace_id, database_id, name, type, config_json, position,
                            created_at, updated_at, link_id)
SELECT id, workspace_id, database_id, name, type, config_json, position, created_at, updated_at, NULL
FROM database_views_backup;

DROP TABLE database_views_backup;

CREATE INDEX database_views_database ON database_views (database_id, position, id);
CREATE INDEX database_views_workspace ON database_views (workspace_id);
CREATE INDEX database_views_link ON database_views (link_id, position, id);

-- A view of a link belongs to the link's database.
CREATE TRIGGER database_views_scope_insert
BEFORE INSERT ON database_views
WHEN NOT EXISTS (
    SELECT 1 FROM pages
    WHERE pages.id = NEW.database_id AND pages.workspace_id = NEW.workspace_id AND pages.kind = 'database'
) OR (NEW.link_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM database_links
    WHERE database_links.id = NEW.link_id AND database_links.database_id = NEW.database_id
))
BEGIN
    SELECT RAISE(ABORT, 'database view scope mismatch');
END;

CREATE TRIGGER database_views_scope_immutable
BEFORE UPDATE OF workspace_id, database_id, link_id ON database_views
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.database_id <> OLD.database_id
    OR NEW.link_id IS NOT OLD.link_id
BEGIN
    SELECT RAISE(ABORT, 'database view scope is immutable');
END;
