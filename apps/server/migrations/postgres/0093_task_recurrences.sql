-- Recurring tasks, as in migrations/0093_task_recurrences.sql.
CREATE TABLE task_recurrences (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    frequency TEXT NOT NULL CHECK (frequency IN ('daily', 'weekly', 'monthly', 'yearly')),
    interval_count BIGINT NOT NULL CHECK (interval_count BETWEEN 1 AND 365),
    weekdays BIGINT NOT NULL DEFAULT 0 CHECK (weekdays BETWEEN 0 AND 127),
    base TEXT NOT NULL CHECK (base IN ('due', 'completion')),
    anchor_at BIGINT NOT NULL,
    timezone TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX task_recurrences_workspace ON task_recurrences (workspace_id);
ALTER TABLE tasks ADD COLUMN recurrence_id TEXT REFERENCES task_recurrences(id) ON DELETE SET NULL;
ALTER TABLE tasks ADD COLUMN recurrence_next_id TEXT;
CREATE INDEX tasks_recurrence_pending ON tasks (workspace_id, recurrence_id)
    WHERE recurrence_id IS NOT NULL AND recurrence_next_id IS NULL;
