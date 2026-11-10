-- Each member's own order of the projects in the Tasks sidebar, as in migrations/0093_project_positions.sql.
CREATE TABLE project_positions (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    position BIGINT NOT NULL CHECK (position >= 0),
    PRIMARY KEY (user_id, project_id)
);
CREATE INDEX project_positions_user ON project_positions (workspace_id, user_id, position);
