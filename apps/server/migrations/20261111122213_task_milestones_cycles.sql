-- Milestones, cycles and estimates (issue #88). Postgres: postgres/20261111122213_task_milestones_cycles.sql.
--
-- Both tables belong to one project, like labels. Dates are epoch milliseconds, like tasks.due_at.
-- A cycle covers [starts_at, ends_at): the server rejects overlapping cycles of one project, and
-- cycles that touch (one ends when the next starts) do not overlap.
--
-- The task columns are added in place (no rebuild of tasks, .ai/lessons.md "RESTRICT"). A
-- REFERENCES column added under foreign_keys=ON must default to NULL, which it does. ON DELETE
-- SET NULL fires the UPDATE triggers below with a NULL value, so they guard on IS NOT NULL.

CREATE TABLE task_milestones (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    target_at INTEGER,
    position INTEGER NOT NULL DEFAULT 0,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (project_id, name)
);

CREATE INDEX task_milestones_project_position ON task_milestones (project_id, position, id);

CREATE TABLE task_cycles (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    number INTEGER NOT NULL CHECK (number > 0),
    name TEXT,
    starts_at INTEGER NOT NULL,
    ends_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (project_id, number),
    CHECK (ends_at > starts_at)
);

CREATE INDEX task_cycles_project_starts ON task_cycles (project_id, starts_at, id);

CREATE TRIGGER task_milestones_validate_scope_insert
BEFORE INSERT ON task_milestones
WHEN NOT EXISTS (
    SELECT 1 FROM projects
    WHERE projects.id = NEW.project_id AND projects.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task milestone project must belong to the workspace');
END;

CREATE TRIGGER task_milestones_scope_immutable
BEFORE UPDATE OF workspace_id, project_id ON task_milestones
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.project_id <> OLD.project_id
BEGIN
    SELECT RAISE(ABORT, 'task milestone scope is immutable');
END;

CREATE TRIGGER task_cycles_validate_scope_insert
BEFORE INSERT ON task_cycles
WHEN NOT EXISTS (
    SELECT 1 FROM projects
    WHERE projects.id = NEW.project_id AND projects.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task cycle project must belong to the workspace');
END;

CREATE TRIGGER task_cycles_scope_immutable
BEFORE UPDATE OF workspace_id, project_id ON task_cycles
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.project_id <> OLD.project_id
BEGIN
    SELECT RAISE(ABORT, 'task cycle scope is immutable');
END;

ALTER TABLE tasks ADD COLUMN milestone_id TEXT REFERENCES task_milestones(id) ON DELETE SET NULL;
ALTER TABLE tasks ADD COLUMN cycle_id TEXT REFERENCES task_cycles(id) ON DELETE SET NULL;
ALTER TABLE tasks ADD COLUMN estimate INTEGER CHECK (estimate IS NULL OR estimate >= 0);

-- Serve the ON DELETE SET NULL lookups, the progress counts and the filters.
CREATE INDEX tasks_milestone_idx ON tasks (milestone_id) WHERE milestone_id IS NOT NULL;
CREATE INDEX tasks_cycle_idx ON tasks (cycle_id) WHERE cycle_id IS NOT NULL;

CREATE TRIGGER tasks_milestone_validate_insert
BEFORE INSERT ON tasks
WHEN NEW.milestone_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM task_milestones
    WHERE task_milestones.id = NEW.milestone_id AND task_milestones.project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'task milestone must belong to the task project');
END;

CREATE TRIGGER tasks_milestone_validate_update
BEFORE UPDATE OF project_id, milestone_id ON tasks
WHEN NEW.milestone_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM task_milestones
    WHERE task_milestones.id = NEW.milestone_id AND task_milestones.project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'task milestone must belong to the task project');
END;

CREATE TRIGGER tasks_cycle_validate_insert
BEFORE INSERT ON tasks
WHEN NEW.cycle_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM task_cycles
    WHERE task_cycles.id = NEW.cycle_id AND task_cycles.project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'task cycle must belong to the task project');
END;

CREATE TRIGGER tasks_cycle_validate_update
BEFORE UPDATE OF project_id, cycle_id ON tasks
WHEN NEW.cycle_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM task_cycles
    WHERE task_cycles.id = NEW.cycle_id AND task_cycles.project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'task cycle must belong to the task project');
END;
