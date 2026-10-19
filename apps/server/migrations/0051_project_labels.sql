-- Labels belong to one project, like statuses.
--
-- Every workspace label is copied into every project of its workspace (soft-deleted projects
-- too, so a restore keeps its labels). The copy in the workspace's oldest live project keeps
-- the label id; the others get new ids. Each task_labels row moves to the copy in the task's
-- own project, so no task loses a label. Saved views keep the old ids: label filters read an
-- id as its name, so they still find every copy.
--
-- SQLite cannot change a UNIQUE constraint or add a NOT NULL reference, so labels and
-- task_labels are rebuilt as in 0021: the rows are copied aside and the tables recreated under
-- the same name. task_labels.label_id becomes ON DELETE CASCADE: a rebuilt labels table is the
-- newest child of projects and workspaces, so their delete cascades reach it before tasks, and
-- a RESTRICT reference to it would block a project or workspace purge.
PRAGMA defer_foreign_keys = ON;

CREATE TABLE labels_backup AS SELECT * FROM labels;
CREATE TABLE task_labels_backup AS SELECT * FROM task_labels;
DROP TABLE task_labels;
DROP TABLE labels;

-- Ids are canonical lowercase UUIDv7: 48-bit millisecond timestamp, version 7, RFC 4122 variant.
CREATE TABLE label_copies AS
SELECT
    seed.old_id,
    seed.project_id,
    CASE WHEN seed.project_id = seed.first_project_id THEN seed.old_id
    ELSE substr(seed.ts, 1, 8) || '-' || substr(seed.ts, 9, 4) || '-7' || substr(seed.r, 1, 3) || '-'
        || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(seed.r, 4, 3) || '-'
        || substr(seed.r, 7, 12)
    END AS new_id
FROM (
    SELECT
        labels_backup.id AS old_id,
        projects.id AS project_id,
        (SELECT first.id FROM projects AS first
         WHERE first.workspace_id = labels_backup.workspace_id
         ORDER BY first.deleted_at IS NOT NULL, first.created_at, first.id LIMIT 1) AS first_project_id,
        printf('%012x', CAST(strftime('%s', 'now') AS INTEGER) * 1000
            + CAST(substr(strftime('%f', 'now'), 4, 3) AS INTEGER)) AS ts,
        lower(hex(randomblob(9))) AS r
    FROM labels_backup
    JOIN projects ON projects.workspace_id = labels_backup.workspace_id
) AS seed;

CREATE TABLE labels (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    color TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (project_id, name)
);

INSERT INTO labels (id, workspace_id, project_id, name, color, version, created_at, updated_at)
SELECT label_copies.new_id, labels_backup.workspace_id, label_copies.project_id,
       labels_backup.name, labels_backup.color, labels_backup.version,
       labels_backup.created_at, labels_backup.updated_at
FROM label_copies
JOIN labels_backup ON labels_backup.id = label_copies.old_id;

CREATE TABLE task_labels (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    label_id TEXT NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, label_id)
);

INSERT INTO task_labels (task_id, label_id)
SELECT task_labels_backup.task_id, label_copies.new_id
FROM task_labels_backup
JOIN tasks ON tasks.id = task_labels_backup.task_id
JOIN label_copies ON label_copies.old_id = task_labels_backup.label_id
    AND label_copies.project_id = tasks.project_id;

DROP TABLE label_copies;
DROP TABLE task_labels_backup;
DROP TABLE labels_backup;

CREATE INDEX labels_workspace_name ON labels (workspace_id, name, id);
CREATE INDEX labels_project_name ON labels (project_id, name, id);
CREATE INDEX task_labels_label_task ON task_labels (label_id, task_id);

CREATE TRIGGER labels_validate_scope_insert
BEFORE INSERT ON labels
WHEN NOT EXISTS (
    SELECT 1 FROM projects
    WHERE projects.id = NEW.project_id AND projects.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'label project must belong to the workspace');
END;

CREATE TRIGGER labels_scope_immutable
BEFORE UPDATE OF workspace_id, project_id ON labels
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.project_id <> OLD.project_id
BEGIN
    SELECT RAISE(ABORT, 'label scope is immutable');
END;

CREATE TRIGGER task_labels_validate_scope_insert
BEFORE INSERT ON task_labels
WHEN NOT EXISTS (
    SELECT 1 FROM tasks JOIN labels ON labels.id = NEW.label_id
    WHERE tasks.id = NEW.task_id AND labels.project_id = tasks.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'task label must belong to the task project');
END;

CREATE TRIGGER task_labels_validate_scope_update
BEFORE UPDATE OF task_id, label_id ON task_labels
WHEN NOT EXISTS (
    SELECT 1 FROM tasks JOIN labels ON labels.id = NEW.label_id
    WHERE tasks.id = NEW.task_id AND labels.project_id = tasks.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'task label must belong to the task project');
END;
