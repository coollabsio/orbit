ALTER TABLE tasks ADD COLUMN due_at INTEGER;
CREATE INDEX tasks_workspace_due ON tasks (workspace_id, due_at, id);
