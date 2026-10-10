-- Task subscribers, subscriber notifications, and inbox actions (snooze, archive).
PRAGMA defer_foreign_keys = ON;

-- 1. Task subscribers

-- Who gets the later events of a task. `subscribed = 0` records a manual unsubscribe: the
-- automatic rules for the creator and for comment authors leave such a row alone, while a new
-- assignment or mention sets it back to 1.
CREATE TABLE task_subscribers (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    subscribed INTEGER NOT NULL CHECK (subscribed IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (task_id, user_id)
);

-- For the cascade when a user is deleted.
CREATE INDEX task_subscribers_user ON task_subscribers (user_id);

-- The creator (a task that an integration made has none), the assignees and the comment
-- authors of every task that is not in the trash.
INSERT OR IGNORE INTO task_subscribers (task_id, user_id, subscribed, created_at, updated_at)
SELECT id, creator_id, 1, created_at, created_at FROM tasks
WHERE deleted_at IS NULL AND creator_service_account_id IS NULL;

INSERT OR IGNORE INTO task_subscribers (task_id, user_id, subscribed, created_at, updated_at)
SELECT task_assignees.task_id, task_assignees.user_id, 1, tasks.created_at, tasks.created_at
FROM task_assignees JOIN tasks ON tasks.id = task_assignees.task_id
WHERE tasks.deleted_at IS NULL;

INSERT OR IGNORE INTO task_subscribers (task_id, user_id, subscribed, created_at, updated_at)
SELECT task_comments.task_id, task_comments.author_id, 1, tasks.created_at, tasks.created_at
FROM task_comments JOIN tasks ON tasks.id = task_comments.task_id
WHERE tasks.deleted_at IS NULL;

-- 2. Notifications
--
-- The last rebuild for kinds: `kind` is free text (the server's `NotificationKind` enum is the
-- list), and the per-kind shape CHECKs become one generic CHECK: at most one target family
-- (task, page or chat message). Zero is allowed, so a later target column needs only
-- `ALTER TABLE ADD COLUMN`; the server gives every row a target.
--
-- A task has one row per recipient now (the inbox shows a task once): a new event updates the
-- row. So `comment_id` is SET NULL on delete (a deleted comment must not take the task's row
-- along), and the actor is optional (a change that GitHub made has no user).
--
-- Rebuilt like in 0037: the runner holds a transaction with foreign_keys=ON, nothing
-- references notifications, rows are copied aside and back.
CREATE TABLE notifications_backup AS SELECT * FROM notifications;
CREATE INDEX notifications_backup_task ON notifications_backup (task_id, recipient_user_id);
DROP TABLE notifications;

CREATE TABLE notifications (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    recipient_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    actor_user_id TEXT REFERENCES users(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 64),
    task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    comment_id TEXT REFERENCES task_comments(id) ON DELETE SET NULL,
    page_id TEXT REFERENCES pages(id) ON DELETE CASCADE,
    page_thread_id TEXT REFERENCES page_threads(id) ON DELETE CASCADE,
    page_comment_id TEXT REFERENCES page_comments(id) ON DELETE CASCADE,
    page_block_id TEXT CHECK (page_block_id IS NULL OR length(page_block_id) BETWEEN 1 AND 64),
    chat_conversation_id TEXT REFERENCES chat_conversations(id) ON DELETE CASCADE,
    chat_message_id TEXT REFERENCES chat_messages(id) ON DELETE CASCADE,
    dedupe_key TEXT NOT NULL UNIQUE,
    read_at INTEGER,
    -- The time of the row's latest event.
    created_at INTEGER NOT NULL,
    pushed_at INTEGER,
    -- Hidden from the inbox until this time; a new event clears it.
    snoozed_until INTEGER,
    archived_at INTEGER,
    CHECK ((task_id IS NOT NULL) + (page_id IS NOT NULL) + (chat_conversation_id IS NOT NULL) <= 1)
);

INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, page_id,
                           page_thread_id, page_comment_id, page_block_id, chat_conversation_id,
                           chat_message_id, dedupe_key, read_at, created_at, pushed_at)
SELECT id, workspace_id, recipient_user_id, actor_user_id, kind, page_id,
       page_thread_id, page_comment_id, page_block_id, chat_conversation_id,
       chat_message_id, dedupe_key, read_at, created_at, pushed_at
FROM notifications_backup WHERE task_id IS NULL;

-- Task rows merge into the latest one of each (recipient, task); it is unread if one of them was.
INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, task_id,
                           comment_id, dedupe_key, read_at, created_at, pushed_at)
SELECT id, workspace_id, recipient_user_id, actor_user_id, kind, task_id, comment_id,
       workspace_id || ':' || recipient_user_id || ':task:' || task_id,
       CASE WHEN EXISTS (
           SELECT 1 FROM notifications_backup AS unread
           WHERE unread.task_id = latest.task_id
             AND unread.recipient_user_id = latest.recipient_user_id
             AND unread.read_at IS NULL
       ) THEN NULL ELSE read_at END,
       created_at, pushed_at
FROM notifications_backup AS latest
WHERE task_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM notifications_backup AS newer
    WHERE newer.task_id = latest.task_id
      AND newer.recipient_user_id = latest.recipient_user_id
      AND (newer.created_at > latest.created_at
           OR (newer.created_at = latest.created_at AND newer.id > latest.id))
);

DROP TABLE notifications_backup;

CREATE INDEX notifications_inbox ON notifications (
    workspace_id,
    recipient_user_id,
    created_at,
    id
);
CREATE INDEX notifications_task ON notifications (task_id);
CREATE INDEX notifications_comment ON notifications (comment_id);
CREATE INDEX notifications_page ON notifications (page_id);
CREATE INDEX notifications_page_thread ON notifications (page_thread_id);
CREATE INDEX notifications_page_comment ON notifications (page_comment_id);
CREATE INDEX notifications_recipient ON notifications (recipient_user_id);
CREATE INDEX notifications_actor ON notifications (actor_user_id);
CREATE INDEX notifications_chat_message ON notifications (chat_message_id);
-- Reading a conversation marks its unread mention notifications as read.
CREATE INDEX notifications_chat_unread ON notifications (chat_conversation_id, recipient_user_id)
    WHERE read_at IS NULL AND chat_conversation_id IS NOT NULL;
-- The 10-minute dedupe looks up the recipient's latest page mention of a page.
CREATE INDEX notifications_page_mentions ON notifications (page_id, recipient_user_id, created_at)
    WHERE kind = 'page_mentioned';
CREATE INDEX notifications_unpushed ON notifications (id) WHERE pushed_at IS NULL;

CREATE TRIGGER notifications_validate_scope_insert
BEFORE INSERT ON notifications
WHEN NOT EXISTS (
    SELECT 1 FROM memberships
    WHERE memberships.workspace_id = NEW.workspace_id
      AND memberships.user_id = NEW.recipient_user_id
) OR (
    NEW.task_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM tasks
        WHERE tasks.id = NEW.task_id AND tasks.workspace_id = NEW.workspace_id
    )
) OR (
    NEW.comment_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM task_comments
        WHERE task_comments.id = NEW.comment_id
          AND task_comments.task_id = NEW.task_id
          AND task_comments.workspace_id = NEW.workspace_id
    )
) OR (
    NEW.page_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM pages
        WHERE pages.id = NEW.page_id
          AND pages.workspace_id = NEW.workspace_id
          AND (pages.owner_id IS NULL OR pages.owner_id = NEW.recipient_user_id)
    )
) OR (
    NEW.chat_message_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM chat_messages
        JOIN chat_conversations ON chat_conversations.id = chat_messages.conversation_id
        WHERE chat_messages.id = NEW.chat_message_id
          AND chat_messages.conversation_id = NEW.chat_conversation_id
          AND chat_conversations.workspace_id = NEW.workspace_id
    )
) OR (
    NEW.page_comment_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM page_comments
        WHERE page_comments.id = NEW.page_comment_id
          AND page_comments.thread_id = NEW.page_thread_id
          AND page_comments.page_id = NEW.page_id
          AND page_comments.workspace_id = NEW.workspace_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'notification must belong to the workspace task, a page the recipient can see or a chat message');
END;

-- 3. Notification preferences
--
-- One row per category a user turned off; no row means on. A new category needs no migration
-- (the server's `Category` enum is the list).
CREATE TABLE notification_pref_overrides (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    category TEXT NOT NULL CHECK (length(category) BETWEEN 1 AND 64),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, category)
);

INSERT INTO notification_pref_overrides (user_id, category, enabled, updated_at)
SELECT user_id, 'direct_messages', 0, updated_at FROM notification_prefs WHERE direct_messages = 0
UNION ALL
SELECT user_id, 'chat_mentions', 0, updated_at FROM notification_prefs WHERE chat_mentions = 0
UNION ALL
SELECT user_id, 'thread_replies', 0, updated_at FROM notification_prefs WHERE thread_replies = 0
UNION ALL
SELECT user_id, 'channel_messages', 0, updated_at FROM notification_prefs WHERE channel_messages = 0
UNION ALL
SELECT user_id, 'task_assigned', 0, updated_at FROM notification_prefs WHERE task_assigned = 0
UNION ALL
SELECT user_id, 'mentions', 0, updated_at FROM notification_prefs WHERE mentions = 0;

DROP TABLE notification_prefs;
