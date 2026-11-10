-- Docs databases: row templates, as in migrations/0084_database_templates.sql.
CREATE TABLE database_templates (
    page_id TEXT PRIMARY KEY REFERENCES pages(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    placeholders_json TEXT NOT NULL DEFAULT '{}',
    position BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX database_templates_database ON database_templates (database_id, position);
CREATE INDEX database_templates_workspace ON database_templates (workspace_id);

CREATE FUNCTION database_templates_scope_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pages
        JOIN pages AS db ON db.id = pages.parent_id
        WHERE pages.id = NEW.page_id AND pages.workspace_id = NEW.workspace_id
        AND db.id = NEW.database_id AND db.workspace_id = NEW.workspace_id AND db.kind = 'database'
    )
    THEN
        RAISE EXCEPTION 'database template scope mismatch';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER database_templates_scope_insert
    BEFORE INSERT ON database_templates
    FOR EACH ROW
    EXECUTE FUNCTION database_templates_scope_insert_fn();
CREATE FUNCTION database_templates_scope_immutable_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'database template scope is immutable';
END;
$$;
CREATE TRIGGER database_templates_scope_immutable
    BEFORE UPDATE OF page_id, workspace_id, database_id ON database_templates
    FOR EACH ROW
    WHEN (NEW.page_id <> OLD.page_id OR NEW.workspace_id <> OLD.workspace_id
        OR NEW.database_id <> OLD.database_id)
    EXECUTE FUNCTION database_templates_scope_immutable_fn();

