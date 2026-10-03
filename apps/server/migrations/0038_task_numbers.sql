-- Per-project task numbers (Linear style `ENG-12`). `projects.task_counter` is the last number
-- handed out; it only grows, so a number is never reused after a delete or a move.
ALTER TABLE projects ADD COLUMN task_counter INTEGER NOT NULL DEFAULT 0 CHECK (task_counter >= 0);
ALTER TABLE tasks ADD COLUMN number INTEGER CHECK (number IS NULL OR number > 0);

UPDATE tasks
SET number = ranked.number
FROM (
    SELECT id, ROW_NUMBER() OVER (PARTITION BY project_id ORDER BY created_at, id) AS number
    FROM tasks
) AS ranked
WHERE ranked.id = tasks.id;

UPDATE projects
SET task_counter = COALESCE((SELECT MAX(number) FROM tasks WHERE tasks.project_id = projects.id), 0);

CREATE UNIQUE INDEX tasks_project_number ON tasks (project_id, number);

-- The triggers cover every writer (API, MCP, integrations, seeds). An insert without a number
-- takes the project's next one; an explicit number (an import) only moves the counter forward.
CREATE TRIGGER tasks_number_insert
AFTER INSERT ON tasks
WHEN NEW.number IS NULL
BEGIN
    UPDATE projects SET task_counter = task_counter + 1 WHERE id = NEW.project_id;
    UPDATE tasks SET number = (SELECT task_counter FROM projects WHERE id = NEW.project_id)
    WHERE id = NEW.id;
END;

CREATE TRIGGER tasks_number_insert_explicit
AFTER INSERT ON tasks
WHEN NEW.number IS NOT NULL
BEGIN
    UPDATE projects SET task_counter = MAX(task_counter, NEW.number) WHERE id = NEW.project_id;
END;

-- A task moved to another project takes that project's next number. The moving UPDATE should
-- clear `number` in the same statement (`number = NULL`), or the old number may collide with
-- the unique index before this trigger runs.
CREATE TRIGGER tasks_number_project_change
AFTER UPDATE OF project_id ON tasks
WHEN NEW.project_id IS NOT OLD.project_id
BEGIN
    UPDATE projects SET task_counter = task_counter + 1 WHERE id = NEW.project_id;
    UPDATE tasks SET number = (SELECT task_counter FROM projects WHERE id = NEW.project_id)
    WHERE id = NEW.id;
END;
