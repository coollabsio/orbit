-- Docs databases (P2 item 5): row templates. A template is a hidden child page of its database:
-- its body is a normal page, its values normal page_property_values. Every query that reads the
-- rows of a database leaves template pages out. `placeholders_json` maps a property id to "me"
-- (a person value adds the person who makes the row) or "today" (a date value is the day the row
-- is made); the stored values never hold these literals.
CREATE TABLE database_templates (
    page_id TEXT PRIMARY KEY REFERENCES pages(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    placeholders_json TEXT NOT NULL DEFAULT '{}',
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX database_templates_database ON database_templates (database_id, position);
CREATE INDEX database_templates_workspace ON database_templates (workspace_id);

-- The template page is a child of the database page, all in one workspace.
CREATE TRIGGER database_templates_scope_insert
BEFORE INSERT ON database_templates
WHEN NOT EXISTS (
    SELECT 1 FROM pages
    JOIN pages AS db ON db.id = pages.parent_id
    WHERE pages.id = NEW.page_id AND pages.workspace_id = NEW.workspace_id
    AND db.id = NEW.database_id AND db.workspace_id = NEW.workspace_id AND db.kind = 'database'
)
BEGIN
    SELECT RAISE(ABORT, 'database template scope mismatch');
END;

CREATE TRIGGER database_templates_scope_immutable
BEFORE UPDATE OF page_id, workspace_id, database_id ON database_templates
WHEN NEW.page_id <> OLD.page_id OR NEW.workspace_id <> OLD.workspace_id
    OR NEW.database_id <> OLD.database_id
BEGIN
    SELECT RAISE(ABORT, 'database template scope is immutable');
END;

