-- Postgres: the same change as ../20261112211451_project_role_grants.sql (issue #73).

CREATE TABLE workspace_role_projects (
    role_id TEXT NOT NULL REFERENCES workspace_roles(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    PRIMARY KEY (role_id, project_id, permission)
);

CREATE INDEX workspace_role_projects_project ON workspace_role_projects (project_id);
