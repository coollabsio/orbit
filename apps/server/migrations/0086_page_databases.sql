-- Docs databases (P1): a page of kind 'database' holds typed properties, saved views and rows. A row
-- is a page whose parent is a database page; its property values live in page_property_values. The
-- title and the created/last edited properties store no value: they read the row's page columns.
ALTER TABLE pages ADD COLUMN kind TEXT NOT NULL DEFAULT 'page' CHECK (kind IN ('page', 'database'));

CREATE TRIGGER pages_kind_immutable
BEFORE UPDATE OF kind ON pages
WHEN NEW.kind <> OLD.kind
BEGIN
    SELECT RAISE(ABORT, 'page kind is immutable');
END;

CREATE TABLE database_properties (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    -- One of the P1 property types; checked in Rust.
    type TEXT NOT NULL,
    config_json TEXT NOT NULL DEFAULT '{}',
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX database_properties_database ON database_properties (database_id, position, id);
CREATE INDEX database_properties_workspace ON database_properties (workspace_id);

CREATE TABLE database_views (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    type TEXT NOT NULL CHECK (type IN ('table')),
    config_json TEXT NOT NULL DEFAULT '{}',
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX database_views_database ON database_views (database_id, position, id);
CREATE INDEX database_views_workspace ON database_views (workspace_id);

-- A missing row means "empty"; values are never stored as null.
CREATE TABLE page_property_values (
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    property_id TEXT NOT NULL REFERENCES database_properties(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    value_json TEXT NOT NULL,
    updated_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (page_id, property_id)
);
CREATE INDEX page_property_values_property ON page_property_values (property_id);
CREATE INDEX page_property_values_workspace ON page_property_values (workspace_id);
CREATE INDEX page_property_values_updated_by ON page_property_values (updated_by);

CREATE TRIGGER database_properties_scope_insert
BEFORE INSERT ON database_properties
WHEN NOT EXISTS (
    SELECT 1 FROM pages
    WHERE pages.id = NEW.database_id AND pages.workspace_id = NEW.workspace_id AND pages.kind = 'database'
)
BEGIN
    SELECT RAISE(ABORT, 'database property scope mismatch');
END;

CREATE TRIGGER database_properties_scope_immutable
BEFORE UPDATE OF workspace_id, database_id ON database_properties
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.database_id <> OLD.database_id
BEGIN
    SELECT RAISE(ABORT, 'database property scope is immutable');
END;

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

-- The value's page must be a row of the property's database, all in one workspace.
CREATE TRIGGER page_property_values_scope_insert
BEFORE INSERT ON page_property_values
WHEN NOT EXISTS (
    SELECT 1 FROM pages
    JOIN database_properties ON database_properties.database_id = pages.parent_id
    WHERE pages.id = NEW.page_id AND pages.workspace_id = NEW.workspace_id
    AND database_properties.id = NEW.property_id AND database_properties.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'page property value scope mismatch');
END;

CREATE TRIGGER page_property_values_scope_immutable
BEFORE UPDATE OF page_id, property_id, workspace_id ON page_property_values
WHEN NEW.page_id <> OLD.page_id OR NEW.property_id <> OLD.property_id
    OR NEW.workspace_id <> OLD.workspace_id
BEGIN
    SELECT RAISE(ABORT, 'page property value scope is immutable');
END;
