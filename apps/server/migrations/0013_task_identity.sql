ALTER TABLE projects ADD COLUMN next_task_number INTEGER NOT NULL DEFAULT 1;
ALTER TABLE tasks ADD COLUMN identifier_key TEXT;
ALTER TABLE tasks ADD COLUMN number INTEGER;

UPDATE tasks
SET identifier_key = (
    SELECT COALESCE(projects.restore_project_key, projects.project_key)
    FROM projects
    WHERE projects.id = tasks.project_id
);

UPDATE tasks
SET number = (
    SELECT COUNT(*)
    FROM tasks AS earlier
    WHERE earlier.workspace_id = tasks.workspace_id
      AND earlier.identifier_key = tasks.identifier_key
      AND (
          earlier.created_at < tasks.created_at
          OR (earlier.created_at = tasks.created_at AND earlier.id <= tasks.id)
      )
);

UPDATE projects
SET next_task_number = 1 + COALESCE((
    SELECT MAX(tasks.number)
    FROM tasks
    WHERE tasks.workspace_id = projects.workspace_id
      AND tasks.identifier_key = COALESCE(projects.restore_project_key, projects.project_key)
), 0);

CREATE UNIQUE INDEX tasks_identifier ON tasks (workspace_id, identifier_key, number);

CREATE TRIGGER tasks_require_identifier_insert
BEFORE INSERT ON tasks
WHEN NEW.identifier_key IS NULL OR NEW.number IS NULL
BEGIN
    SELECT RAISE(ABORT, 'task identifier key and number are required');
END;

CREATE TRIGGER tasks_require_identifier_update
BEFORE UPDATE OF identifier_key, number ON tasks
WHEN NEW.identifier_key IS NULL OR NEW.number IS NULL
BEGIN
    SELECT RAISE(ABORT, 'task identifier key and number are required');
END;
