-- Intake (Linear parity, area 5): the `backlog` and `triage` status categories, the triage
-- setting, task templates and recurring tasks.
--
-- SQLite cannot alter a CHECK constraint, so task_statuses is rebuilt as in 0021: the runner
-- holds a transaction with foreign_keys=ON, so the rows are copied aside, the table is dropped
-- and created again under the same name, and every id is inserted again. defer_foreign_keys
-- moves the tasks.status_id check (ON DELETE RESTRICT) to COMMIT; if a status id were lost,
-- COMMIT fails and the migration rolls back.
--
-- The implicit DELETE of DROP TABLE runs the foreign key actions of every table that refers to
-- task_statuses. At this migration these are:
--   tasks.status_id                       ON DELETE RESTRICT  (deferred, see above)
--   task_relations.previous_status_id     ON DELETE SET NULL  (the values would be lost)
--   project_pr_automation.status_id       ON DELETE CASCADE   (the rows would be deleted)
-- so both are copied aside and written back below. A new table that refers to task_statuses
-- must get the same treatment here.
PRAGMA defer_foreign_keys = ON;

CREATE TABLE task_statuses_backup AS SELECT * FROM task_statuses;
CREATE TABLE task_relations_status_backup AS
    SELECT id, previous_status_id FROM task_relations WHERE previous_status_id IS NOT NULL;

CREATE TABLE project_pr_automation_backup AS
    SELECT project_id, event, status_id FROM project_pr_automation WHERE status_id IS NOT NULL;

DROP TABLE task_statuses;

CREATE TABLE task_statuses (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    color TEXT NOT NULL,
    category TEXT NOT NULL
        CHECK (category IN ('triage', 'backlog', 'unstarted', 'started', 'completed', 'cancelled',
                            'duplicate')),
    position INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category,
                           position, version, created_at, updated_at)
SELECT id, workspace_id, project_id, name, description, color, category,
       position, version, created_at, updated_at
FROM task_statuses_backup;

UPDATE task_relations SET previous_status_id = (
    SELECT backup.previous_status_id FROM task_relations_status_backup AS backup
    WHERE backup.id = task_relations.id
)
WHERE id IN (SELECT id FROM task_relations_status_backup);

-- The cascade deleted these rows; OR REPLACE also covers a row that is still there.
INSERT OR REPLACE INTO project_pr_automation (project_id, event, status_id)
SELECT project_id, event, status_id FROM project_pr_automation_backup;

DROP TABLE project_pr_automation_backup;
DROP TABLE task_relations_status_backup;
DROP TABLE task_statuses_backup;

CREATE INDEX task_statuses_project_position ON task_statuses (workspace_id, project_id, position, id);

CREATE TRIGGER task_statuses_validate_scope_insert
BEFORE INSERT ON task_statuses
WHEN NOT EXISTS (
    SELECT 1 FROM projects
    WHERE projects.id = NEW.project_id AND projects.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task status project must belong to the workspace');
END;

CREATE TRIGGER task_statuses_validate_scope_update
BEFORE UPDATE OF workspace_id, project_id ON task_statuses
WHEN NOT EXISTS (
    SELECT 1 FROM projects
    WHERE projects.id = NEW.project_id AND projects.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task status project must belong to the workspace');
END;

CREATE TRIGGER task_statuses_scope_immutable
BEFORE UPDATE OF workspace_id, project_id ON task_statuses
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.project_id <> OLD.project_id
BEGIN
    SELECT RAISE(ABORT, 'task status scope is immutable');
END;

-- One system Duplicate status and one system Triage status per project; neither can be
-- re-categorised, and no other status can take their category.
CREATE UNIQUE INDEX task_statuses_one_duplicate
    ON task_statuses (project_id) WHERE category = 'duplicate';
CREATE UNIQUE INDEX task_statuses_one_triage
    ON task_statuses (project_id) WHERE category = 'triage';

CREATE TRIGGER task_statuses_duplicate_category_immutable
BEFORE UPDATE OF category ON task_statuses
WHEN (OLD.category = 'duplicate') <> (NEW.category = 'duplicate')
BEGIN
    SELECT RAISE(ABORT, 'the duplicate status category is immutable');
END;

CREATE TRIGGER task_statuses_triage_category_immutable
BEFORE UPDATE OF category ON task_statuses
WHEN (OLD.category = 'triage') <> (NEW.category = 'triage')
BEGIN
    SELECT RAISE(ABORT, 'the triage status category is immutable');
END;

-- Seed a Triage status for every existing project (soft-deleted ones too, so restore works).
-- Ids are canonical lowercase UUIDv7: 48-bit millisecond timestamp, version 7, RFC 4122 variant.
INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category,
                           position, version, created_at, updated_at)
SELECT
    substr(seed.ts, 1, 8) || '-' || substr(seed.ts, 9, 4) || '-7' || substr(seed.r, 1, 3) || '-'
        || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(seed.r, 4, 3) || '-'
        || substr(seed.r, 7, 12),
    seed.workspace_id,
    seed.project_id,
    'Triage',
    '',
    '#f2994a',
    'triage',
    seed.position,
    0,
    seed.now_ms,
    seed.now_ms
FROM (
    SELECT
        projects.id AS project_id,
        projects.workspace_id AS workspace_id,
        (SELECT COALESCE(MAX(task_statuses.position) + 1, 0) FROM task_statuses
         WHERE task_statuses.project_id = projects.id) AS position,
        CAST(strftime('%s', 'now') AS INTEGER) * 1000
            + CAST(substr(strftime('%f', 'now'), 4, 3) AS INTEGER) AS now_ms,
        printf('%012x', CAST(strftime('%s', 'now') AS INTEGER) * 1000
            + CAST(substr(strftime('%f', 'now'), 4, 3) AS INTEGER)) AS ts,
        lower(hex(randomblob(9))) AS r
    FROM projects
) AS seed;

-- Triage is a setting of the project, off by default: tasks that come from an integration wait
-- in the Triage status until a member accepts them.
ALTER TABLE projects ADD COLUMN triage_enabled INTEGER NOT NULL DEFAULT 0
    CHECK (triage_enabled IN (0, 1));

-- A template holds the fields of a task (and its sub-issues) as JSON, so a new task field needs
-- no migration. The server checks every reference again when the payload is used.
CREATE TABLE task_templates (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    position INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (project_id, name)
);

CREATE INDEX task_templates_project_position ON task_templates (project_id, position, id);

-- `mode` (schedule | after_completion) and `every_unit` (day | week | month) are checked in
-- server code. `next_run_at` is NULL while an after_completion routine waits for its last task.
CREATE TABLE recurring_tasks (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    payload_json TEXT NOT NULL,
    mode TEXT NOT NULL,
    every_count INTEGER NOT NULL CHECK (every_count >= 1),
    every_unit TEXT NOT NULL,
    anchor_at INTEGER NOT NULL,
    next_run_at INTEGER,
    last_task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL,
    paused INTEGER NOT NULL DEFAULT 0 CHECK (paused IN (0, 1)),
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX recurring_tasks_project ON recurring_tasks (project_id, created_at, id);
CREATE INDEX recurring_tasks_next_run ON recurring_tasks (next_run_at) WHERE paused = 0;
