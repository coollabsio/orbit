-- A token without a task scope has no project, so `project_id` may be NULL. A set project must
-- still be active and belong to the token's workspace. Recreating the insert trigger makes it
-- the newest, so SQLite now runs it before `api_tokens_validate_service_account_insert`.
DROP TRIGGER api_tokens_validate_project_insert;
DROP TRIGGER api_tokens_validate_project_update;

CREATE TRIGGER api_tokens_validate_project_insert
BEFORE INSERT ON api_tokens
WHEN NEW.project_id IS NOT NULL AND NOT EXISTS (
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
WHEN NEW.project_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM projects
    WHERE projects.id = NEW.project_id
      AND projects.workspace_id = NEW.workspace_id
      AND projects.deleted_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'api token project must be active and belong to the workspace');
END;
