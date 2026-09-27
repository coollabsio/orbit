-- Saved task views, per-user view favorites, per-page view preferences, and tasks.completed_at.
--
-- state_json holds a ViewState ({ filter, display }) that the server validates
-- (apps/server/src/repositories/task_filter.rs); the schema only bounds human-entered fields.
-- Users are never hard-deleted, so user references need no ON DELETE action. A workspace purge
-- cascades through saved_views (and from there saved_view_favorites) and view_preferences; none
-- of these tables is referenced with RESTRICT, so the cascade order does not matter.

CREATE TABLE saved_views (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    owner_user_id TEXT NOT NULL REFERENCES users(id),
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 80),
    description TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 500),
    icon TEXT,
    color TEXT,
    visibility TEXT NOT NULL CHECK (visibility IN ('personal', 'workspace')),
    state_json TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX saved_views_workspace_visibility ON saved_views (workspace_id, visibility);
CREATE INDEX saved_views_workspace_owner ON saved_views (workspace_id, owner_user_id);

CREATE TABLE saved_view_favorites (
    view_id TEXT NOT NULL REFERENCES saved_views(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id),
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (view_id, user_id)
);

-- The sidebar lists one user's favorites in order.
CREATE INDEX saved_view_favorites_user ON saved_view_favorites (user_id, position);

CREATE TABLE view_preferences (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id),
    page_key TEXT NOT NULL,
    state_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, user_id, page_key)
);

-- When the task last entered a done category (completed, cancelled, duplicate); NULL while open.
ALTER TABLE tasks ADD COLUMN completed_at INTEGER;

UPDATE tasks SET completed_at = updated_at
WHERE status_id IN (
    SELECT id FROM task_statuses WHERE category IN ('completed', 'cancelled', 'duplicate')
);

CREATE INDEX tasks_workspace_completed ON tasks (workspace_id, completed_at, id);

-- The triggers cover every writer (UI, bulk, GitHub sync, duplicate marking). Every statement
-- that writes status_id also sets updated_at, so NEW.updated_at is the moment of the change.
-- Re-writing the same status (every PATCH does) or moving between two done categories keeps the
-- original completed_at. Category edits on task_statuses do not rewrite completed_at (v1).
CREATE TRIGGER tasks_completed_at_insert
AFTER INSERT ON tasks
WHEN NEW.completed_at IS NULL AND EXISTS (
    SELECT 1 FROM task_statuses
    WHERE task_statuses.id = NEW.status_id
      AND task_statuses.category IN ('completed', 'cancelled', 'duplicate')
)
BEGIN
    UPDATE tasks SET completed_at = NEW.updated_at WHERE id = NEW.id;
END;

CREATE TRIGGER tasks_completed_at_update
AFTER UPDATE OF status_id ON tasks
WHEN EXISTS (
    SELECT 1 FROM task_statuses
    WHERE task_statuses.id = NEW.status_id
      AND task_statuses.category IN ('completed', 'cancelled', 'duplicate')
) <> EXISTS (
    SELECT 1 FROM task_statuses
    WHERE task_statuses.id = OLD.status_id
      AND task_statuses.category IN ('completed', 'cancelled', 'duplicate')
)
BEGIN
    UPDATE tasks
    SET completed_at = CASE
        WHEN EXISTS (
            SELECT 1 FROM task_statuses
            WHERE task_statuses.id = NEW.status_id
              AND task_statuses.category IN ('completed', 'cancelled', 'duplicate')
        ) THEN NEW.updated_at
        ELSE NULL
    END
    WHERE id = NEW.id;
END;
