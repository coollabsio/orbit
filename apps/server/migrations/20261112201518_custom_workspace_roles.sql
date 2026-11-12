-- SQLite. Postgres: postgres/20261112201518_custom_workspace_roles.sql.
--
-- Custom workspace roles (issue #73). A custom role is a named list of extra permissions that
-- an ordinary member holds on top of the member role. `permissions` is a JSON array of
-- permission names; the server drops any name that a custom role may not carry. A member holds at
-- most one custom role, only while their base role is `member`. Deleting a role takes it from
-- its members.

CREATE TABLE workspace_roles (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    permissions TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX workspace_roles_name ON workspace_roles (workspace_id, lower(name));

ALTER TABLE memberships ADD COLUMN custom_role_id TEXT REFERENCES workspace_roles(id) ON DELETE SET NULL;

-- Serves the ON DELETE SET NULL lookup and the member count of a role.
CREATE INDEX memberships_custom_role ON memberships (custom_role_id) WHERE custom_role_id IS NOT NULL;
