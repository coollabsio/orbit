-- Identifiers a task had before it moved to another project (Linear style: `ENG-12` keeps
-- leading to the task after it became `OPS-4`). Only the identifier resolver reads this table,
-- and only when no live task holds the number. Numbers are never reused, so an alias normally
-- has one owner for good; an imported explicit number that later moves away takes it over.
CREATE TABLE task_number_aliases (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    number INTEGER NOT NULL CHECK (number > 0),
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    PRIMARY KEY (project_id, number)
) WITHOUT ROWID;

CREATE INDEX task_number_aliases_task ON task_number_aliases (task_id);

-- Replaces the trigger of 0039. `OLD` holds the row as it was before the moving UPDATE, so
-- `OLD.number` is the old number even though that UPDATE clears `number` itself.
DROP TRIGGER tasks_number_project_change;

CREATE TRIGGER tasks_number_project_change
AFTER UPDATE OF project_id ON tasks
WHEN NEW.project_id IS NOT OLD.project_id
BEGIN
    INSERT OR REPLACE INTO task_number_aliases (project_id, number, task_id)
    SELECT OLD.project_id, OLD.number, OLD.id WHERE OLD.number IS NOT NULL;
    UPDATE projects SET task_counter = task_counter + 1 WHERE id = NEW.project_id;
    UPDATE tasks SET number = (SELECT task_counter FROM projects WHERE id = NEW.project_id)
    WHERE id = NEW.id;
END;
