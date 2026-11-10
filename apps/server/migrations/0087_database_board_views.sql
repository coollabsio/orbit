-- Docs databases: board views. A board groups the rows into columns by a select or status
-- property; its settings live in config_json next to the table settings.
--
-- SQLite cannot alter a CHECK constraint, so database_views is rebuilt as in 0021: the runner holds
-- a transaction with foreign_keys=ON, so the rows are copied aside, the table is dropped and
-- recreated under the same name, and the rows come back under defer_foreign_keys. No table
-- references database_views, so the DROP cascades nowhere. Its indexes and triggers (0079) are
-- recreated below.
PRAGMA defer_foreign_keys = ON;

CREATE TABLE database_views_backup AS SELECT * FROM database_views;
DROP TABLE database_views;

CREATE TABLE database_views (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    type TEXT NOT NULL CHECK (type IN ('table', 'board')),
    config_json TEXT NOT NULL DEFAULT '{}',
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

INSERT INTO database_views (id, workspace_id, database_id, name, type, config_json, position,
                            created_at, updated_at)
SELECT id, workspace_id, database_id, name, type, config_json, position, created_at, updated_at
FROM database_views_backup;

DROP TABLE database_views_backup;

CREATE INDEX database_views_database ON database_views (database_id, position, id);
CREATE INDEX database_views_workspace ON database_views (workspace_id);

CREATE TRIGGER database_views_scope_insert
BEFORE INSERT ON database_views
WHEN NOT EXISTS (
    SELECT 1 FROM pages
    WHERE pages.id = NEW.database_id AND pages.workspace_id = NEW.workspace_id AND pages.kind = 'database'
)
BEGIN
    SELECT RAISE(ABORT, 'database view scope mismatch');
END;

CREATE TRIGGER database_views_scope_immutable
BEFORE UPDATE OF workspace_id, database_id ON database_views
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.database_id <> OLD.database_id
BEGIN
    SELECT RAISE(ABORT, 'database view scope is immutable');
END;
