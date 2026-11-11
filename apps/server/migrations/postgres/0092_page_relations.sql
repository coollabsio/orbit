-- Docs database relations, as in migrations/0092_page_relations.sql.
CREATE TABLE page_relations (
    property_id TEXT NOT NULL REFERENCES database_properties(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    related_page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    position BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (property_id, page_id, related_page_id)
);
CREATE INDEX page_relations_related ON page_relations (related_page_id, property_id);
CREATE INDEX page_relations_workspace ON page_relations (workspace_id);
