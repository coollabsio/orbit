-- Postgres: the same change as ../20261112201518_custom_workspace_roles.sql (issue #73).

CREATE TABLE workspace_roles (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    permissions TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX workspace_roles_name ON workspace_roles (workspace_id, lower(name));

ALTER TABLE memberships ADD COLUMN custom_role_id TEXT REFERENCES workspace_roles(id) ON DELETE SET NULL;

CREATE INDEX memberships_custom_role ON memberships (custom_role_id) WHERE custom_role_id IS NOT NULL;
