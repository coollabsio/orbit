-- Additional MCP permissions default to no access for existing credentials.
ALTER TABLE api_tokens ADD COLUMN mcp_scopes TEXT NOT NULL DEFAULT '';
ALTER TABLE oauth_grants ADD COLUMN mcp_scopes TEXT NOT NULL DEFAULT '';
ALTER TABLE oauth_requests ADD COLUMN mcp_scopes TEXT NOT NULL DEFAULT '';
