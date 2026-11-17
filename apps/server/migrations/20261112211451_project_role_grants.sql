-- SQLite. Postgres: postgres/20261112211451_project_role_grants.sql.
--
-- A custom role can hold a project-scoped permission for single projects (issue #73). One row
-- per role, project and permission name. The role's `permissions` column stays the
-- workspace-wide grant, which covers every project.

CREATE TABLE workspace_role_projects (
    role_id TEXT NOT NULL REFERENCES workspace_roles(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    PRIMARY KEY (role_id, project_id, permission)
);

-- Serves the ON DELETE CASCADE lookup when a project is deleted for good.
CREATE INDEX workspace_role_projects_project ON workspace_role_projects (project_id);
