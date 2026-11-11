-- Private projects, as in migrations/0093_private_projects.sql.
ALTER TABLE projects ADD COLUMN private BIGINT NOT NULL DEFAULT 0 CHECK (private IN (0, 1));
CREATE TABLE project_members (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (project_id, user_id),
    FOREIGN KEY (workspace_id, user_id) REFERENCES memberships(workspace_id, user_id) ON DELETE CASCADE
);
CREATE INDEX project_members_user ON project_members (workspace_id, user_id);
