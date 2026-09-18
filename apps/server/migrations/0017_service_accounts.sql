CREATE TABLE service_accounts (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL,
    disabled_at INTEGER
);

CREATE INDEX service_accounts_workspace_created
    ON service_accounts (workspace_id, created_at, id);
CREATE UNIQUE INDEX service_accounts_workspace_name
    ON service_accounts (workspace_id, lower(name));

ALTER TABLE api_tokens ADD COLUMN service_account_id TEXT
    REFERENCES service_accounts(id) ON DELETE RESTRICT;

ALTER TABLE tasks ADD COLUMN creator_service_account_id TEXT
    REFERENCES service_accounts(id) ON DELETE RESTRICT;

CREATE TRIGGER api_tokens_validate_service_account_insert
BEFORE INSERT ON api_tokens
WHEN NEW.service_account_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM service_accounts
    WHERE id = NEW.service_account_id
      AND workspace_id = NEW.workspace_id
      AND disabled_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'api token service account must be active and belong to the workspace');
END;

CREATE TRIGGER tasks_validate_service_account_insert
BEFORE INSERT ON tasks
WHEN NEW.creator_service_account_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM service_accounts
    WHERE id = NEW.creator_service_account_id
      AND workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task service account must belong to the workspace');
END;
