-- Add the system-managed 'duplicate' status category and task relations.
--
-- SQLite cannot alter a CHECK constraint, so task_statuses is rebuilt. The migration runner
-- executes inside a transaction with foreign_keys=ON, where PRAGMA foreign_keys cannot change.
-- defer_foreign_keys moves the tasks.status_id check to COMMIT instead: the old rows are copied
-- aside, the table is recreated under the same name, and every id is re-inserted. If any status
-- id were lost, COMMIT fails with a foreign key error and the migration rolls back.
-- ALTER TABLE ... RENAME is deliberately avoided: it fails while triggers on tasks name the
-- dropped table. Only tasks.status_id references task_statuses (ON DELETE RESTRICT), so the
-- implicit DELETE performed by DROP TABLE cascades nowhere. Triggers on other tables that
-- mention task_statuses resolve by name at run time; the table's own index and triggers
-- (0007, 0008) are recreated below.
PRAGMA defer_foreign_keys = ON;

CREATE TABLE task_statuses_backup AS SELECT * FROM task_statuses;
DROP TABLE task_statuses;

CREATE TABLE task_statuses (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    color TEXT NOT NULL,
    category TEXT NOT NULL
        CHECK (category IN ('unstarted', 'started', 'completed', 'cancelled', 'duplicate')),
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

-- One system Duplicate status per project; it can never be re-categorised.
CREATE UNIQUE INDEX task_statuses_one_duplicate
    ON task_statuses (project_id) WHERE category = 'duplicate';

CREATE TRIGGER task_statuses_duplicate_category_immutable
BEFORE UPDATE OF category ON task_statuses
WHEN (OLD.category = 'duplicate') <> (NEW.category = 'duplicate')
BEGIN
    SELECT RAISE(ABORT, 'the duplicate status category is immutable');
END;

-- Seed a Duplicate status for every existing project (soft-deleted ones too, so restore works).
-- Ids are canonical lowercase UUIDv7: 48-bit millisecond timestamp, version 7, RFC 4122 variant.
INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, category,
                           position, version, created_at, updated_at)
SELECT
    substr(seed.ts, 1, 8) || '-' || substr(seed.ts, 9, 4) || '-7' || substr(seed.r, 1, 3) || '-'
        || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(seed.r, 4, 3) || '-'
        || substr(seed.r, 7, 12),
    seed.workspace_id,
    seed.project_id,
    'Duplicate',
    '',
    '#8b8f98',
    'duplicate',
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

CREATE TABLE task_relations (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    related_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    type TEXT NOT NULL CHECK (type IN ('blocks', 'related', 'duplicate')),
    previous_status_id TEXT REFERENCES task_statuses(id) ON DELETE SET NULL,
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    CHECK (task_id <> related_task_id),
    CHECK (type = 'duplicate' OR previous_status_id IS NULL),
    CHECK (type <> 'related' OR task_id < related_task_id)
);

-- One relation per unordered pair, whatever its type (this also rejects blocks 2-cycles).
CREATE UNIQUE INDEX task_relations_pair
    ON task_relations (min(task_id, related_task_id), max(task_id, related_task_id));
-- A task duplicates at most one other task.
CREATE UNIQUE INDEX task_relations_one_duplicate
    ON task_relations (task_id) WHERE type = 'duplicate';
CREATE INDEX task_relations_task ON task_relations (task_id, type);
CREATE INDEX task_relations_related ON task_relations (related_task_id, type);

CREATE TRIGGER task_relations_validate_scope_insert
BEFORE INSERT ON task_relations
WHEN NOT EXISTS (
    SELECT 1 FROM tasks WHERE tasks.id = NEW.task_id AND tasks.workspace_id = NEW.workspace_id
) OR NOT EXISTS (
    SELECT 1 FROM tasks WHERE tasks.id = NEW.related_task_id AND tasks.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task relation tasks must belong to the workspace');
END;

CREATE TRIGGER task_relations_validate_scope_update
BEFORE UPDATE OF workspace_id, task_id, related_task_id ON task_relations
WHEN NOT EXISTS (
    SELECT 1 FROM tasks WHERE tasks.id = NEW.task_id AND tasks.workspace_id = NEW.workspace_id
) OR NOT EXISTS (
    SELECT 1 FROM tasks WHERE tasks.id = NEW.related_task_id AND tasks.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task relation tasks must belong to the workspace');
END;

-- Duplicates never chain: the target is not itself a duplicate, and nothing points at the duplicate.
CREATE TRIGGER task_relations_duplicate_chain_insert
BEFORE INSERT ON task_relations
WHEN NEW.type = 'duplicate' AND (
    EXISTS (SELECT 1 FROM task_relations
            WHERE task_relations.task_id = NEW.related_task_id AND task_relations.type = 'duplicate')
    OR EXISTS (SELECT 1 FROM task_relations
               WHERE task_relations.related_task_id = NEW.task_id AND task_relations.type = 'duplicate')
)
BEGIN
    SELECT RAISE(ABORT, 'duplicate relations cannot chain');
END;

CREATE TRIGGER task_relations_duplicate_chain_update
BEFORE UPDATE OF task_id, related_task_id, type ON task_relations
WHEN NEW.type = 'duplicate' AND (
    EXISTS (SELECT 1 FROM task_relations
            WHERE task_relations.id <> NEW.id
              AND task_relations.task_id = NEW.related_task_id AND task_relations.type = 'duplicate')
    OR EXISTS (SELECT 1 FROM task_relations
               WHERE task_relations.id <> NEW.id
                 AND task_relations.related_task_id = NEW.task_id AND task_relations.type = 'duplicate')
)
BEGIN
    SELECT RAISE(ABORT, 'duplicate relations cannot chain');
END;
