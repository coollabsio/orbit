-- Docs databases: inline database blocks and linked views, as in migrations/0080_database_links.sql.
CREATE TABLE database_links (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    host_page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX database_links_database ON database_links (database_id);
CREATE INDEX database_links_host ON database_links (host_page_id);
CREATE INDEX database_links_workspace ON database_links (workspace_id);
CREATE INDEX database_links_created_by ON database_links (created_by);

CREATE FUNCTION database_links_scope_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pages
        WHERE pages.id = NEW.database_id AND pages.workspace_id = NEW.workspace_id AND pages.kind = 'database'
    ) OR NOT EXISTS (
        SELECT 1 FROM pages WHERE pages.id = NEW.host_page_id AND pages.workspace_id = NEW.workspace_id
    )
    THEN
        RAISE EXCEPTION 'database link scope mismatch';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER database_links_scope_insert
    BEFORE INSERT ON database_links
    FOR EACH ROW
    EXECUTE FUNCTION database_links_scope_insert_fn();
CREATE FUNCTION database_links_scope_immutable_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'database link scope is immutable';
END;
$$;
CREATE TRIGGER database_links_scope_immutable
    BEFORE UPDATE OF workspace_id, database_id, host_page_id ON database_links
    FOR EACH ROW
    WHEN (NEW.workspace_id <> OLD.workspace_id OR NEW.database_id <> OLD.database_id
        OR NEW.host_page_id <> OLD.host_page_id)
    EXECUTE FUNCTION database_links_scope_immutable_fn();

ALTER TABLE database_views ADD COLUMN link_id TEXT REFERENCES database_links(id) ON DELETE CASCADE;
ALTER TABLE database_views DROP CONSTRAINT database_views_type_check;
ALTER TABLE database_views ADD CONSTRAINT database_views_type_check
    CHECK (type IN ('table', 'board', 'list', 'calendar', 'gallery', 'timeline', 'form'));
CREATE INDEX database_views_link ON database_views (link_id, position, id);

CREATE OR REPLACE FUNCTION database_views_scope_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pages
        WHERE pages.id = NEW.database_id AND pages.workspace_id = NEW.workspace_id AND pages.kind = 'database'
    ) OR (NEW.link_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM database_links
        WHERE database_links.id = NEW.link_id AND database_links.database_id = NEW.database_id
    ))
    THEN
        RAISE EXCEPTION 'database view scope mismatch';
    END IF;
    RETURN NEW;
END;
$$;
DROP TRIGGER database_views_scope_immutable ON database_views;
CREATE TRIGGER database_views_scope_immutable
    BEFORE UPDATE OF workspace_id, database_id, link_id ON database_views
    FOR EACH ROW
    WHEN (NEW.workspace_id <> OLD.workspace_id OR NEW.database_id <> OLD.database_id
        OR NEW.link_id IS DISTINCT FROM OLD.link_id)
    EXECUTE FUNCTION database_views_scope_immutable_fn();
