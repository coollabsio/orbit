CREATE TABLE api_token_projects (
    token_id TEXT NOT NULL REFERENCES api_tokens(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    PRIMARY KEY (token_id, project_id)
);

INSERT INTO api_token_projects (token_id, project_id)
SELECT id, project_id FROM api_tokens WHERE project_id IS NOT NULL;

CREATE TRIGGER api_token_projects_validate_insert
BEFORE INSERT ON api_token_projects
WHEN NOT EXISTS (
    SELECT 1
    FROM api_tokens
    JOIN projects ON projects.id = NEW.project_id
    WHERE api_tokens.id = NEW.token_id
      AND projects.workspace_id = api_tokens.workspace_id
      AND projects.deleted_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'api token project must be active and belong to the workspace');
END;
