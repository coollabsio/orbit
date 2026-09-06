CREATE TABLE notifications (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    recipient_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    actor_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL CHECK (kind IN ('task_assigned', 'comment_mentioned')),
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    comment_id TEXT REFERENCES task_comments(id) ON DELETE CASCADE,
    dedupe_key TEXT NOT NULL UNIQUE,
    read_at INTEGER,
    created_at INTEGER NOT NULL
);

CREATE INDEX notifications_inbox ON notifications (
    workspace_id,
    recipient_user_id,
    created_at,
    id
);

CREATE TRIGGER notifications_validate_scope_insert
BEFORE INSERT ON notifications
WHEN NOT EXISTS (
    SELECT 1 FROM memberships
    WHERE memberships.workspace_id = NEW.workspace_id
      AND memberships.user_id = NEW.recipient_user_id
) OR NOT EXISTS (
    SELECT 1 FROM tasks
    WHERE tasks.id = NEW.task_id AND tasks.workspace_id = NEW.workspace_id
) OR (
    NEW.comment_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM task_comments
        WHERE task_comments.id = NEW.comment_id
          AND task_comments.task_id = NEW.task_id
          AND task_comments.workspace_id = NEW.workspace_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'notification must belong to the workspace task and recipient');
END;
