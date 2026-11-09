-- Docs databases (P1), as in migrations/0079_page_databases.sql.
ALTER TABLE pages ADD COLUMN kind TEXT NOT NULL DEFAULT 'page' CHECK (kind IN ('page', 'database'));

CREATE FUNCTION pages_kind_immutable_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'page kind is immutable';
END;
$$;
CREATE TRIGGER pages_kind_immutable
    BEFORE UPDATE OF kind ON pages
    FOR EACH ROW
    WHEN (NEW.kind <> OLD.kind)
    EXECUTE FUNCTION pages_kind_immutable_fn();

CREATE TABLE database_properties (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    type TEXT NOT NULL,
    config_json TEXT NOT NULL DEFAULT '{}',
    position BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
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
    position BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX database_views_database ON database_views (database_id, position, id);
CREATE INDEX database_views_workspace ON database_views (workspace_id);

CREATE TABLE page_property_values (
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    property_id TEXT NOT NULL REFERENCES database_properties(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    value_json TEXT NOT NULL,
    updated_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (page_id, property_id)
);
CREATE INDEX page_property_values_property ON page_property_values (property_id);
CREATE INDEX page_property_values_workspace ON page_property_values (workspace_id);
CREATE INDEX page_property_values_updated_by ON page_property_values (updated_by);

CREATE FUNCTION database_properties_scope_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pages
        WHERE pages.id = NEW.database_id AND pages.workspace_id = NEW.workspace_id AND pages.kind = 'database'
    )
    THEN
        RAISE EXCEPTION 'database property scope mismatch';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER database_properties_scope_insert
    BEFORE INSERT ON database_properties
    FOR EACH ROW
    EXECUTE FUNCTION database_properties_scope_insert_fn();
CREATE FUNCTION database_properties_scope_immutable_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'database property scope is immutable';
END;
$$;
CREATE TRIGGER database_properties_scope_immutable
    BEFORE UPDATE OF workspace_id, database_id ON database_properties
    FOR EACH ROW
    WHEN (NEW.workspace_id <> OLD.workspace_id OR NEW.database_id <> OLD.database_id)
    EXECUTE FUNCTION database_properties_scope_immutable_fn();

CREATE FUNCTION database_views_scope_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pages
        WHERE pages.id = NEW.database_id AND pages.workspace_id = NEW.workspace_id AND pages.kind = 'database'
    )
    THEN
        RAISE EXCEPTION 'database view scope mismatch';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER database_views_scope_insert
    BEFORE INSERT ON database_views
    FOR EACH ROW
    EXECUTE FUNCTION database_views_scope_insert_fn();
CREATE FUNCTION database_views_scope_immutable_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'database view scope is immutable';
END;
$$;
CREATE TRIGGER database_views_scope_immutable
    BEFORE UPDATE OF workspace_id, database_id ON database_views
    FOR EACH ROW
    WHEN (NEW.workspace_id <> OLD.workspace_id OR NEW.database_id <> OLD.database_id)
    EXECUTE FUNCTION database_views_scope_immutable_fn();

CREATE FUNCTION page_property_values_scope_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pages
        JOIN database_properties ON database_properties.database_id = pages.parent_id
        WHERE pages.id = NEW.page_id AND pages.workspace_id = NEW.workspace_id
        AND database_properties.id = NEW.property_id AND database_properties.workspace_id = NEW.workspace_id
    )
    THEN
        RAISE EXCEPTION 'page property value scope mismatch';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER page_property_values_scope_insert
    BEFORE INSERT ON page_property_values
    FOR EACH ROW
    EXECUTE FUNCTION page_property_values_scope_insert_fn();
CREATE FUNCTION page_property_values_scope_immutable_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'page property value scope is immutable';
END;
$$;
CREATE TRIGGER page_property_values_scope_immutable
    BEFORE UPDATE OF page_id, property_id, workspace_id ON page_property_values
    FOR EACH ROW
    WHEN (NEW.page_id <> OLD.page_id OR NEW.property_id <> OLD.property_id
        OR NEW.workspace_id <> OLD.workspace_id)
    EXECUTE FUNCTION page_property_values_scope_immutable_fn();
