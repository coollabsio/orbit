-- Postgres: the same change as apps/server/migrations/20261112175303_teamspace_projects.sql
ALTER TABLE teamspaces ADD COLUMN project_id TEXT REFERENCES projects(id) ON DELETE SET NULL;

CREATE UNIQUE INDEX teamspaces_project ON teamspaces (project_id) WHERE project_id IS NOT NULL;
