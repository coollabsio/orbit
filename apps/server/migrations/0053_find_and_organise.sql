-- Find and organise (Area 7): task search, favourites, label groups and archive, in one file.

-- 1. Task search
--
-- Task full-text search: an FTS5 index over the title, the description and the joined comment
-- bodies of each task, with the tokenizer of `page_search` (0027). One row for each task;
-- triggers on `tasks` and `task_comments` keep it in sync. It holds trashed tasks too (queries
-- apply the visibility rules), so trash and restore need no index work.
--
-- FTS rowids come from `task_search_rows`: `tasks` has a TEXT primary key and VACUUM may
-- renumber implicit rowids (see 0027). The control characters U+0002/U+0003 are stripped from
-- indexed text: search uses them as markers in snippet() output.
CREATE TABLE task_search_rows (
    id INTEGER PRIMARY KEY,
    task_id TEXT NOT NULL UNIQUE
);

CREATE VIRTUAL TABLE task_search USING fts5(
    title,
    description,
    comments,
    tokenize = 'unicode61 remove_diacritics 2'
);

INSERT INTO task_search_rows (task_id) SELECT id FROM tasks ORDER BY created_at, id;

INSERT INTO task_search (rowid, title, description, comments)
SELECT
    task_search_rows.id,
    replace(replace(tasks.title, char(2), ''), char(3), ''),
    replace(replace(tasks.description, char(2), ''), char(3), ''),
    COALESCE((
        SELECT group_concat(replace(replace(body, char(2), ''), char(3), ''), char(10))
        FROM (SELECT body FROM task_comments WHERE task_id = tasks.id ORDER BY created_at, id)
    ), '')
FROM task_search_rows JOIN tasks ON tasks.id = task_search_rows.task_id;

CREATE TRIGGER task_search_insert
AFTER INSERT ON tasks
BEGIN
    INSERT INTO task_search_rows (task_id) VALUES (NEW.id);
    INSERT INTO task_search (rowid, title, description, comments)
    VALUES (
        (SELECT id FROM task_search_rows WHERE task_id = NEW.id),
        replace(replace(NEW.title, char(2), ''), char(3), ''),
        replace(replace(NEW.description, char(2), ''), char(3), ''),
        ''
    );
END;

CREATE TRIGGER task_search_update
AFTER UPDATE OF title, description ON tasks
WHEN NEW.title IS NOT OLD.title OR NEW.description IS NOT OLD.description
BEGIN
    UPDATE task_search
    SET title = replace(replace(NEW.title, char(2), ''), char(3), ''),
        description = replace(replace(NEW.description, char(2), ''), char(3), '')
    WHERE rowid = (SELECT id FROM task_search_rows WHERE task_id = NEW.id);
END;

-- Fires for direct deletes (trash purge) and for the project/workspace cascades alike. BEFORE the
-- delete, so the cascade that deletes the comments of the task finds no row to rebuild.
CREATE TRIGGER task_search_delete
BEFORE DELETE ON tasks
BEGIN
    DELETE FROM task_search
    WHERE rowid = (SELECT id FROM task_search_rows WHERE task_id = OLD.id);
    DELETE FROM task_search_rows WHERE task_id = OLD.id;
END;

-- The comments column is the joined text of the task's comments, rebuilt on each comment change.
-- When the task itself is deleted, its row is gone (or goes next) and these updates match nothing.
CREATE TRIGGER task_search_comment_insert
AFTER INSERT ON task_comments
BEGIN
    UPDATE task_search
    SET comments = COALESCE((
        SELECT group_concat(replace(replace(body, char(2), ''), char(3), ''), char(10))
        FROM (SELECT body FROM task_comments WHERE task_id = NEW.task_id ORDER BY created_at, id)
    ), '')
    WHERE rowid = (SELECT id FROM task_search_rows WHERE task_id = NEW.task_id);
END;

CREATE TRIGGER task_search_comment_update
AFTER UPDATE OF body ON task_comments
WHEN NEW.body IS NOT OLD.body
BEGIN
    UPDATE task_search
    SET comments = COALESCE((
        SELECT group_concat(replace(replace(body, char(2), ''), char(3), ''), char(10))
        FROM (SELECT body FROM task_comments WHERE task_id = NEW.task_id ORDER BY created_at, id)
    ), '')
    WHERE rowid = (SELECT id FROM task_search_rows WHERE task_id = NEW.task_id);
END;

CREATE TRIGGER task_search_comment_delete
AFTER DELETE ON task_comments
BEGIN
    UPDATE task_search
    SET comments = COALESCE((
        SELECT group_concat(replace(replace(body, char(2), ''), char(3), ''), char(10))
        FROM (SELECT body FROM task_comments WHERE task_id = OLD.task_id ORDER BY created_at, id)
    ), '')
    WHERE rowid = (SELECT id FROM task_search_rows WHERE task_id = OLD.task_id);
END;

-- 2. Favourites
--
-- One table for the favourites of each kind (`task`, `view`, `project`, `milestone`; the server
-- checks the kind, so a new kind needs no migration). `target_id` has no foreign key: the read query joins each kind
-- to its table and drops rows whose item is gone, in the trash or not visible. The delete
-- triggers remove the rows on each hard delete path (purge and cascades).
CREATE TABLE favorites (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id),
    kind TEXT NOT NULL,
    target_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, kind, target_id)
);

-- The sidebar lists one user's favourites of a workspace in order.
CREATE INDEX favorites_user_position ON favorites (workspace_id, user_id, position);
CREATE INDEX favorites_target ON favorites (kind, target_id);

INSERT INTO favorites (workspace_id, user_id, kind, target_id, position, created_at)
SELECT saved_views.workspace_id, saved_view_favorites.user_id, 'view', saved_view_favorites.view_id,
       saved_view_favorites.position, saved_view_favorites.created_at
FROM saved_view_favorites JOIN saved_views ON saved_views.id = saved_view_favorites.view_id;

DROP TABLE saved_view_favorites;

CREATE TRIGGER favorites_task_delete
AFTER DELETE ON tasks
BEGIN
    DELETE FROM favorites WHERE kind = 'task' AND target_id = OLD.id;
END;

CREATE TRIGGER favorites_view_delete
AFTER DELETE ON saved_views
BEGIN
    DELETE FROM favorites WHERE kind = 'view' AND target_id = OLD.id;
END;

CREATE TRIGGER favorites_project_delete
AFTER DELETE ON projects
BEGIN
    DELETE FROM favorites WHERE kind = 'project' AND target_id = OLD.id;
END;

CREATE TRIGGER favorites_milestone_delete
AFTER DELETE ON milestones
BEGIN
    DELETE FROM favorites WHERE kind = 'milestone' AND target_id = OLD.id;
END;

-- 3. Label groups
--
-- A label can belong to one group, and a task has not more than one label of a group. The
-- trigger keeps the rule on each write path (task edits, bulk edits, integrations): a new label
-- of a group replaces the task's other label of that group, in the same transaction. A label
-- cannot join a group while a task would break the rule; the server checks that (409).
CREATE TABLE label_groups (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    color TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (workspace_id, name)
);

ALTER TABLE labels ADD COLUMN group_id TEXT REFERENCES label_groups(id) ON DELETE SET NULL;

CREATE INDEX labels_group ON labels (group_id);

CREATE TRIGGER task_labels_one_per_group
AFTER INSERT ON task_labels
WHEN (SELECT group_id FROM labels WHERE id = NEW.label_id) IS NOT NULL
BEGIN
    DELETE FROM task_labels
    WHERE task_id = NEW.task_id AND label_id <> NEW.label_id
      AND label_id IN (
          SELECT id FROM labels
          WHERE group_id = (SELECT group_id FROM labels WHERE id = NEW.label_id)
      );
END;

-- 4. Archive
--
-- An archived task leaves the default lists and the search; it is not deleted. A closed tree
-- (a top-level task with all its sub-issues) is archived together: each task of it gets the
-- same `archived_root_id`, so a restore finds the tree with no recursion. A project can archive
-- its closed trees automatically after `auto_archive_months` (NULL = off).
ALTER TABLE tasks ADD COLUMN archived_at INTEGER;
ALTER TABLE tasks ADD COLUMN archived_root_id TEXT;
ALTER TABLE projects ADD COLUMN auto_archive_months INTEGER
    CHECK (auto_archive_months IS NULL OR auto_archive_months IN (3, 6, 12));

CREATE INDEX tasks_archived_root ON tasks (archived_root_id) WHERE archived_root_id IS NOT NULL;

-- A reopened task must be visible: a move to an open status restores the archived tree. The
-- trigger covers each write path of `status_id` (edits, bulk edits, duplicates, GitHub sync).
CREATE TRIGGER tasks_unarchive_on_reopen
AFTER UPDATE OF status_id ON tasks
WHEN NEW.archived_at IS NOT NULL AND EXISTS (
    SELECT 1 FROM task_statuses
    WHERE task_statuses.id = NEW.status_id
      AND task_statuses.category NOT IN ('completed', 'cancelled', 'duplicate')
)
BEGIN
    UPDATE tasks
    SET archived_at = NULL, archived_root_id = NULL, version = version + 1
    WHERE archived_root_id = NEW.archived_root_id OR id = NEW.id;
END;

-- The tree rule holds after the archive too: an open task that joins an archived tree (a new
-- sub-issue, or a task that gets an archived parent) restores that tree, and an archived task that
-- gets a different parent restores the tree it was archived with.
CREATE TRIGGER tasks_unarchive_on_open_child_insert
AFTER INSERT ON tasks
WHEN NEW.parent_task_id IS NOT NULL
 AND EXISTS (SELECT 1 FROM tasks AS parent WHERE parent.id = NEW.parent_task_id AND parent.archived_at IS NOT NULL)
 AND EXISTS (
    SELECT 1 FROM task_statuses
    WHERE task_statuses.id = NEW.status_id
      AND task_statuses.category NOT IN ('completed', 'cancelled', 'duplicate')
)
BEGIN
    UPDATE tasks
    SET archived_at = NULL, archived_root_id = NULL, version = version + 1
    WHERE id = NEW.parent_task_id
       OR archived_root_id = (SELECT archived_root_id FROM tasks WHERE id = NEW.parent_task_id);
END;

CREATE TRIGGER tasks_unarchive_on_parent_change
AFTER UPDATE OF parent_task_id ON tasks
WHEN NEW.parent_task_id IS NOT OLD.parent_task_id AND (
    NEW.archived_at IS NOT NULL
    OR (
        EXISTS (SELECT 1 FROM tasks AS parent WHERE parent.id = NEW.parent_task_id AND parent.archived_at IS NOT NULL)
        AND EXISTS (
            SELECT 1 FROM task_statuses
            WHERE task_statuses.id = NEW.status_id
              AND task_statuses.category NOT IN ('completed', 'cancelled', 'duplicate')
        )
    )
)
BEGIN
    UPDATE tasks
    SET archived_at = NULL, archived_root_id = NULL, version = version + 1
    WHERE (NEW.archived_at IS NOT NULL AND (archived_root_id = NEW.archived_root_id OR id = NEW.id))
       OR id = NEW.parent_task_id
       OR archived_root_id = (SELECT archived_root_id FROM tasks WHERE id = NEW.parent_task_id AND archived_at IS NOT NULL);
END;
