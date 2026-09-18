ALTER TABLE api_tokens ADD COLUMN project_id TEXT REFERENCES projects(id) ON DELETE RESTRICT;

UPDATE api_tokens
SET project_id = (
    SELECT projects.id FROM projects
    WHERE projects.workspace_id = api_tokens.workspace_id
    ORDER BY projects.deleted_at IS NOT NULL, projects.created_at, projects.id
    LIMIT 1
);

CREATE TRIGGER api_tokens_validate_project_insert
BEFORE INSERT ON api_tokens
WHEN NEW.project_id IS NULL OR NOT EXISTS (
    SELECT 1 FROM projects
    WHERE projects.id = NEW.project_id
      AND projects.workspace_id = NEW.workspace_id
      AND projects.deleted_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'api token project must be active and belong to the workspace');
END;

CREATE TRIGGER api_tokens_validate_project_update
BEFORE UPDATE OF workspace_id, project_id ON api_tokens
WHEN NEW.project_id IS NULL OR NOT EXISTS (
    SELECT 1 FROM projects
    WHERE projects.id = NEW.project_id
      AND projects.workspace_id = NEW.workspace_id
      AND projects.deleted_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'api token project must be active and belong to the workspace');
END;
