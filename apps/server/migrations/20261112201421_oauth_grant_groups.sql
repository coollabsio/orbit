-- SQLite
-- One MCP connection can cover several workspaces. Each workspace keeps its own grant row (its projects),
-- and the grants of one consent share a group. A token belongs to the first grant and works for the whole
-- group. Grants that exist today are each a group of one.
ALTER TABLE oauth_grants ADD COLUMN group_id TEXT;
UPDATE oauth_grants SET group_id = id;
CREATE INDEX oauth_grants_group ON oauth_grants(group_id);
