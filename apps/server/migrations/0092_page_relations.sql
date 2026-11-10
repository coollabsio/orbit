-- Docs database relations (P3 item 9): each link of a relation property is one row here, stored
-- under the pair's primary property only; the synced property of a two-way pair reads the same rows
-- from the other side (related_page_id). Permanently deleting either page removes its links.
CREATE TABLE page_relations (
    property_id TEXT NOT NULL REFERENCES database_properties(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    related_page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (property_id, page_id, related_page_id)
);
CREATE INDEX page_relations_related ON page_relations (related_page_id, property_id);
CREATE INDEX page_relations_workspace ON page_relations (workspace_id);
