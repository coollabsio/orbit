-- One space-separated scope set per token, with the names OAuth uses (`tasks:read`, `tasks:write`,
-- `docs:read`, `chat:read`), replacing the read/write flags and the separate MCP scope list.
UPDATE api_tokens SET mcp_scopes = trim(
    CASE WHEN can_read = 1 THEN 'tasks:read ' ELSE '' END
    || CASE WHEN can_write = 1 THEN 'tasks:write ' ELSE '' END
    || mcp_scopes
);
ALTER TABLE api_tokens RENAME COLUMN mcp_scopes TO scopes;
ALTER TABLE api_tokens DROP COLUMN can_read;
ALTER TABLE api_tokens DROP COLUMN can_write;
