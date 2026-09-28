-- Sub-issues (docs/superpowers/specs/2026-09-27-sub-issues-design.md §1): a task's direct parent and the
-- per-project auto-close switches.
--
-- ADD COLUMN keeps tasks in place (no rebuild), so the FK child order the retention purge depends on is
-- unchanged (.ai/lessons.md, "RESTRICT"). A REFERENCES column added under foreign_keys=ON must default to
-- NULL, which it does. ON DELETE SET NULL (retention purge, project purge, workspace purge) fires the UPDATE
-- trigger below with NEW.parent_task_id NULL, so both triggers guard on IS NOT NULL. Cycles deeper than a
-- self-reference are rejected by the repository with a recursive CTE, inside the write transaction.
ALTER TABLE tasks ADD COLUMN parent_task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL;

CREATE INDEX tasks_parent_idx ON tasks (parent_task_id) WHERE parent_task_id IS NOT NULL;

CREATE TRIGGER tasks_parent_validate_insert
BEFORE INSERT ON tasks
WHEN NEW.parent_task_id IS NOT NULL AND (
    NEW.parent_task_id = NEW.id
    OR NOT EXISTS (
        SELECT 1 FROM tasks AS parent
        WHERE parent.id = NEW.parent_task_id AND parent.workspace_id = NEW.workspace_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'task parent must be another task in the same workspace');
END;

CREATE TRIGGER tasks_parent_validate_update
BEFORE UPDATE OF parent_task_id ON tasks
WHEN NEW.parent_task_id IS NOT NULL AND (
    NEW.parent_task_id = NEW.id
    OR NOT EXISTS (
        SELECT 1 FROM tasks AS parent
        WHERE parent.id = NEW.parent_task_id AND parent.workspace_id = NEW.workspace_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'task parent must be another task in the same workspace');
END;

-- Rule A (close a parent when all its sub-issues are done) reads the parent's project; rule B (close
-- open sub-issues when their parent closes) reads the closing task's project. Both default on.
ALTER TABLE projects ADD COLUMN auto_close_parent INTEGER NOT NULL DEFAULT 1
    CHECK (auto_close_parent IN (0, 1));
ALTER TABLE projects ADD COLUMN auto_close_sub_issues INTEGER NOT NULL DEFAULT 1
    CHECK (auto_close_sub_issues IN (0, 1));
