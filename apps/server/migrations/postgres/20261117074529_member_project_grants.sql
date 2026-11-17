-- Postgres: the same change as ../20261117074529_member_project_grants.sql (issue #73).
CREATE TABLE member_project_grants (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    PRIMARY KEY (project_id, user_id, permission)
);

CREATE INDEX member_project_grants_user ON member_project_grants (user_id);

INSERT INTO member_project_grants (project_id, user_id, permission)
SELECT id, created_by, 'projects.manage' FROM projects WHERE created_by IS NOT NULL;
