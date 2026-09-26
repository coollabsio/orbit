-- Docs page mentions: an @mention of a member in a page body (BlockNote `mention` inline content)
-- notifies that member once (kind `page_mentioned`): only members who can see the page, never the
-- editor, and at most once per page and recipient within 10 minutes (enforced by the server).
-- `page_block_id` names the block holding the (first new) mention, for a deep link.
--
-- The kind CHECK changes, so the table is rebuilt like in 0030 (SQLite cannot alter a CHECK). The
-- migration runner holds a transaction with foreign_keys=ON; nothing references notifications,
-- so the implicit DELETE of DROP TABLE cascades nowhere. Rows are copied aside and back; the
-- indexes and the trigger are recreated below.
PRAGMA defer_foreign_keys = ON;

CREATE TABLE notifications_backup AS SELECT * FROM notifications;
DROP TABLE notifications;

CREATE TABLE notifications (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    recipient_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    actor_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL
        CHECK (kind IN ('task_assigned', 'comment_mentioned', 'page_comment_mentioned', 'page_mentioned')),
    task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    comment_id TEXT REFERENCES task_comments(id) ON DELETE CASCADE,
    page_id TEXT REFERENCES pages(id) ON DELETE CASCADE,
    page_thread_id TEXT REFERENCES page_threads(id) ON DELETE CASCADE,
    page_comment_id TEXT REFERENCES page_comments(id) ON DELETE CASCADE,
    page_block_id TEXT CHECK (page_block_id IS NULL OR length(page_block_id) BETWEEN 1 AND 64),
    dedupe_key TEXT NOT NULL UNIQUE,
    read_at INTEGER,
    created_at INTEGER NOT NULL,
    CHECK (
        (kind IN ('task_assigned', 'comment_mentioned')
            AND task_id IS NOT NULL
            AND page_id IS NULL AND page_thread_id IS NULL AND page_comment_id IS NULL
            AND page_block_id IS NULL)
        OR (kind = 'page_comment_mentioned'
            AND task_id IS NULL AND comment_id IS NULL
            AND page_id IS NOT NULL AND page_thread_id IS NOT NULL AND page_comment_id IS NOT NULL
            AND page_block_id IS NULL)
        OR (kind = 'page_mentioned'
            AND task_id IS NULL AND comment_id IS NULL
            AND page_id IS NOT NULL AND page_thread_id IS NULL AND page_comment_id IS NULL)
    )
);

INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, task_id,
                           comment_id, page_id, page_thread_id, page_comment_id, dedupe_key,
                           read_at, created_at)
SELECT id, workspace_id, recipient_user_id, actor_user_id, kind, task_id,
       comment_id, page_id, page_thread_id, page_comment_id, dedupe_key,
       read_at, created_at
FROM notifications_backup;

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
-- The 10-minute dedupe looks up the recipient's latest page mention of a page.
CREATE INDEX notifications_page_mentions ON notifications (page_id, recipient_user_id, created_at)
    WHERE kind = 'page_mentioned';

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
    NEW.page_comment_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM page_comments
        WHERE page_comments.id = NEW.page_comment_id
          AND page_comments.thread_id = NEW.page_thread_id
          AND page_comments.page_id = NEW.page_id
          AND page_comments.workspace_id = NEW.workspace_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'notification must belong to the workspace task or a page the recipient can see');
END;
