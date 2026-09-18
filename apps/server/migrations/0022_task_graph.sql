-- Task graph: sub-issues (tasks.parent_id), duplicate relations and derived
-- backlinks harvested from rich text. Nesting is any depth and cross-project,
-- so the only scope rule on parent_id is the workspace.
ALTER TABLE tasks ADD COLUMN parent_id TEXT REFERENCES tasks(id) ON DELETE SET NULL;
CREATE INDEX tasks_parent ON tasks (workspace_id, parent_id, position, id);

-- partial index sized for the progress rollup; covers only LIVE sub-issues
CREATE INDEX tasks_parent_rollup ON tasks (workspace_id, parent_id, status_id)
    WHERE deleted_at IS NULL AND parent_id IS NOT NULL;

CREATE TABLE task_relations (
    id             TEXT PRIMARY KEY,
    workspace_id   TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind           TEXT NOT NULL CHECK (kind IN ('duplicate_of')),
    source_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    target_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    created_by     TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at     INTEGER NOT NULL,
    CHECK (source_task_id <> target_task_id)
);
CREATE UNIQUE INDEX task_relations_one_duplicate
    ON task_relations (source_task_id) WHERE kind = 'duplicate_of';
CREATE INDEX task_relations_target ON task_relations (workspace_id, target_task_id, kind);

-- Derived from rich text on every description/comment write; never audited.
CREATE TABLE task_references (
    workspace_id   TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    source_type    TEXT NOT NULL CHECK (source_type IN ('task', 'comment')),
    source_id      TEXT NOT NULL,
    target_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    PRIMARY KEY (source_type, source_id, target_task_id)
);
CREATE INDEX task_references_target ON task_references (workspace_id, target_task_id);

CREATE TRIGGER tasks_validate_parent_insert
BEFORE INSERT ON tasks
WHEN NEW.parent_id IS NOT NULL AND (
    NEW.parent_id = NEW.id OR NOT EXISTS (
        SELECT 1 FROM tasks parent
        WHERE parent.id = NEW.parent_id AND parent.workspace_id = NEW.workspace_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'task parent must be another task in the same workspace');
END;

CREATE TRIGGER tasks_validate_parent_update
BEFORE UPDATE OF parent_id, workspace_id ON tasks
WHEN NEW.parent_id IS NOT NULL AND (
    NEW.parent_id = NEW.id OR NOT EXISTS (
        SELECT 1 FROM tasks parent
        WHERE parent.id = NEW.parent_id AND parent.workspace_id = NEW.workspace_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'task parent must be another task in the same workspace');
END;

CREATE TRIGGER task_relations_validate_scope_insert
BEFORE INSERT ON task_relations
WHEN NOT EXISTS (
    SELECT 1 FROM tasks
    WHERE tasks.id = NEW.source_task_id AND tasks.workspace_id = NEW.workspace_id
) OR NOT EXISTS (
    SELECT 1 FROM tasks
    WHERE tasks.id = NEW.target_task_id AND tasks.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task relation must join tasks in its workspace');
END;

CREATE TRIGGER task_references_validate_scope_insert
BEFORE INSERT ON task_references
WHEN NOT EXISTS (
    SELECT 1 FROM tasks
    WHERE tasks.id = NEW.target_task_id AND tasks.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task reference target must belong to the workspace');
END;

-- source_id is polymorphic, so it carries no foreign key: drop a source's
-- backlinks when the task or comment that owned them is hard-deleted.
CREATE TRIGGER tasks_cleanup_references
AFTER DELETE ON tasks
BEGIN
    DELETE FROM task_references WHERE source_type = 'task' AND source_id = OLD.id;
END;

CREATE TRIGGER task_comments_cleanup_references
AFTER DELETE ON task_comments
BEGIN
    DELETE FROM task_references WHERE source_type = 'comment' AND source_id = OLD.id;
END;

-- Backfill: documents written since 0014 may already mention tasks. Harvest
-- those mentions once here; from now on the server rebuilds a source's rows on
-- every write. Only same-workspace, existing targets qualify, and a task never
-- references itself.
INSERT OR IGNORE INTO task_references (workspace_id, source_type, source_id, target_task_id)
SELECT source.workspace_id, 'task', source.id, target.id
FROM tasks AS source,
     json_tree(CASE WHEN json_valid(source.description_json) THEN source.description_json ELSE '{}' END) AS node
JOIN tasks AS target
  ON target.id = json_extract(node.value, '$.attrs.id')
 AND target.workspace_id = source.workspace_id
WHERE node.type = 'object'
  AND json_extract(node.value, '$.type') = 'taskMention'
  AND target.id <> source.id;

INSERT OR IGNORE INTO task_references (workspace_id, source_type, source_id, target_task_id)
SELECT comment.workspace_id, 'comment', comment.id, target.id
FROM task_comments AS comment,
     json_tree(CASE WHEN json_valid(comment.body_json) THEN comment.body_json ELSE '{}' END) AS node
JOIN tasks AS target
  ON target.id = json_extract(node.value, '$.attrs.id')
 AND target.workspace_id = comment.workspace_id
WHERE node.type = 'object'
  AND json_extract(node.value, '$.type') = 'taskMention';
