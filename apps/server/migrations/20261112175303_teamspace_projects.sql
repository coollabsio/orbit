-- SQLite
-- A teamspace can hold the docs of one project (#64). Teamspaces without a project hold the
-- workspace's global docs. A project has at most one teamspace; the repository checks that the
-- project is in the teamspace's workspace.
ALTER TABLE teamspaces ADD COLUMN project_id TEXT REFERENCES projects(id) ON DELETE SET NULL;

CREATE UNIQUE INDEX teamspaces_project ON teamspaces (project_id) WHERE project_id IS NOT NULL;
