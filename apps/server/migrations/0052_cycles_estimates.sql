-- Estimates and cycles (Linear parity, area 3). An Orbit project is a product, the equivalent of
-- a Linear team, so the estimate scale and the cycles belong to the project. Every change is an
-- ADD COLUMN or a new table: no table is rebuilt.

-- NULL = estimates are off. The values (fibonacci | linear | tshirt) are checked in server code.
ALTER TABLE projects ADD COLUMN estimate_scale TEXT;

-- Always points, whatever the scale of the project shows. NULL = no estimate.
ALTER TABLE tasks ADD COLUMN estimate INTEGER CHECK (estimate IS NULL OR estimate >= 0);

-- One row for each project that has used cycles.
CREATE TABLE project_cycle_settings (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    weeks INTEGER NOT NULL CHECK (weeks BETWEEN 1 AND 8),
    -- 0 = Sunday ... 6 = Saturday
    start_weekday INTEGER NOT NULL CHECK (start_weekday BETWEEN 0 AND 6),
    cooldown_weeks INTEGER NOT NULL CHECK (cooldown_weeks BETWEEN 0 AND 4),
    cycles_ahead INTEGER NOT NULL CHECK (cycles_ahead BETWEEN 1 AND 15),
    -- IANA name; a cycle starts and ends in this zone, not in the zone of the viewer
    timezone TEXT NOT NULL,
    auto_add_started INTEGER NOT NULL CHECK (auto_add_started IN (0, 1)),
    auto_add_completed INTEGER NOT NULL CHECK (auto_add_completed IN (0, 1)),
    -- off | backlog | cycle (checked in server code)
    active_without_cycle TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    updated_at INTEGER NOT NULL
);

-- A cycle is the period [starts_at, ends_at). The default name is "Cycle {number}".
CREATE TABLE cycles (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    number INTEGER NOT NULL,
    name TEXT,
    description TEXT NOT NULL DEFAULT '',
    starts_at INTEGER NOT NULL,
    ends_at INTEGER NOT NULL,
    completed_at INTEGER,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (project_id, number),
    CHECK (ends_at > starts_at)
);

CREATE INDEX cycles_project_starts ON cycles (project_id, starts_at);

CREATE TRIGGER cycles_validate_scope_insert
BEFORE INSERT ON cycles
WHEN NOT EXISTS (
    SELECT 1 FROM projects
    WHERE projects.id = NEW.project_id AND projects.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'cycle project must belong to the workspace');
END;

CREATE TRIGGER cycles_scope_immutable
BEFORE UPDATE OF workspace_id, project_id ON cycles
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.project_id <> OLD.project_id
BEGIN
    SELECT RAISE(ABORT, 'cycle scope is immutable');
END;

-- A task's cycle must be in the task's project. ON DELETE SET NULL (cycles turned off, project
-- purge) fires the UPDATE trigger with a NULL cycle, so both triggers guard on IS NOT NULL.
ALTER TABLE tasks ADD COLUMN cycle_id TEXT REFERENCES cycles(id) ON DELETE SET NULL;

CREATE INDEX tasks_cycle_idx ON tasks (cycle_id) WHERE cycle_id IS NOT NULL;

CREATE TRIGGER tasks_cycle_validate_insert
BEFORE INSERT ON tasks
WHEN NEW.cycle_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM cycles WHERE cycles.id = NEW.cycle_id AND cycles.project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'task cycle must belong to the task project');
END;

CREATE TRIGGER tasks_cycle_validate_update
BEFORE UPDATE OF project_id, cycle_id ON tasks
WHEN NEW.cycle_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM cycles WHERE cycles.id = NEW.cycle_id AND cycles.project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'task cycle must belong to the task project');
END;

-- One snapshot for each day of a cycle while it is current, and a last one at its close. A
-- burndown cannot be calculated later from current data, so the rows exist from the first cycle.
-- `day` is the local date of the project timezone, as YYYY-MM-DD.
CREATE TABLE cycle_days (
    cycle_id TEXT NOT NULL REFERENCES cycles(id) ON DELETE CASCADE,
    day TEXT NOT NULL,
    scope_count INTEGER NOT NULL,
    scope_points INTEGER NOT NULL,
    started_count INTEGER NOT NULL,
    started_points INTEGER NOT NULL,
    done_count INTEGER NOT NULL,
    done_points INTEGER NOT NULL,
    PRIMARY KEY (cycle_id, day)
);
