-- Milestones, cycles and estimates (issue #88). SQLite: ../20261111122213_task_milestones_cycles.sql.

CREATE TABLE task_milestones (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    target_at BIGINT,
    position BIGINT NOT NULL DEFAULT 0,
    version BIGINT NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE (project_id, name)
);

CREATE INDEX task_milestones_project_position ON task_milestones (project_id, position, id);

CREATE TABLE task_cycles (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    number BIGINT NOT NULL CHECK (number > 0),
    name TEXT,
    starts_at BIGINT NOT NULL,
    ends_at BIGINT NOT NULL,
    version BIGINT NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE (project_id, number),
    CHECK (ends_at > starts_at)
);

CREATE INDEX task_cycles_project_starts ON task_cycles (project_id, starts_at, id);

-- TG_ARGV[0] is the error message, as in SQLite's RAISE.
CREATE FUNCTION task_plan_validate_scope_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM projects
        WHERE projects.id = NEW.project_id AND projects.workspace_id = NEW.workspace_id
    )
    THEN
        RAISE EXCEPTION '%', TG_ARGV[0];
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION task_plan_scope_immutable_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION '%', TG_ARGV[0];
END;
$$;

CREATE TRIGGER task_milestones_validate_scope_insert
    BEFORE INSERT ON task_milestones
    FOR EACH ROW
    EXECUTE FUNCTION task_plan_validate_scope_insert_fn('task milestone project must belong to the workspace');

CREATE TRIGGER task_milestones_scope_immutable
    BEFORE UPDATE OF workspace_id, project_id ON task_milestones
    FOR EACH ROW
    WHEN (NEW.workspace_id <> OLD.workspace_id OR NEW.project_id <> OLD.project_id)
    EXECUTE FUNCTION task_plan_scope_immutable_fn('task milestone scope is immutable');

CREATE TRIGGER task_cycles_validate_scope_insert
    BEFORE INSERT ON task_cycles
    FOR EACH ROW
    EXECUTE FUNCTION task_plan_validate_scope_insert_fn('task cycle project must belong to the workspace');

CREATE TRIGGER task_cycles_scope_immutable
    BEFORE UPDATE OF workspace_id, project_id ON task_cycles
    FOR EACH ROW
    WHEN (NEW.workspace_id <> OLD.workspace_id OR NEW.project_id <> OLD.project_id)
    EXECUTE FUNCTION task_plan_scope_immutable_fn('task cycle scope is immutable');

ALTER TABLE tasks
    ADD COLUMN milestone_id TEXT REFERENCES task_milestones(id) ON DELETE SET NULL,
    ADD COLUMN cycle_id TEXT REFERENCES task_cycles(id) ON DELETE SET NULL,
    ADD COLUMN estimate BIGINT CHECK (estimate IS NULL OR estimate >= 0);

CREATE INDEX tasks_milestone_idx ON tasks (milestone_id) WHERE milestone_id IS NOT NULL;
CREATE INDEX tasks_cycle_idx ON tasks (cycle_id) WHERE cycle_id IS NOT NULL;

CREATE FUNCTION tasks_validate_plan_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.milestone_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM task_milestones
        WHERE task_milestones.id = NEW.milestone_id AND task_milestones.project_id = NEW.project_id
    )
    THEN
        RAISE EXCEPTION 'task milestone must belong to the task project';
    END IF;
    IF NEW.cycle_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM task_cycles
        WHERE task_cycles.id = NEW.cycle_id AND task_cycles.project_id = NEW.project_id
    )
    THEN
        RAISE EXCEPTION 'task cycle must belong to the task project';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER tasks_validate_plan_insert
    BEFORE INSERT ON tasks
    FOR EACH ROW
    EXECUTE FUNCTION tasks_validate_plan_fn();

CREATE TRIGGER tasks_validate_plan_update
    BEFORE UPDATE OF project_id, milestone_id, cycle_id ON tasks
    FOR EACH ROW
    EXECUTE FUNCTION tasks_validate_plan_fn();
