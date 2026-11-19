-- SQLite. Postgres: postgres/20261118130637_chat_reports_and_blocks.sql.
--
-- App Store guideline 1.2 (user content). A member reports a chat message: the report keeps a copy
-- of the body, so the people who act on it can read it after an edit or a delete, and also when
-- they cannot read the conversation (a DM). The members who can manage chat get an inbox
-- notification (kind `chat_reported`). A user blocks another user: the blocker's chat hides the
-- blocked person's messages, and the blocked person cannot open or write a DM with the blocker.
CREATE TABLE chat_message_reports (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
    message_id TEXT REFERENCES chat_messages(id) ON DELETE SET NULL,
    reporter_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    author_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reason TEXT NOT NULL CHECK (reason IN ('spam', 'harassment', 'inappropriate', 'other')),
    note TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 500),
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (message_id, reporter_id)
);

CREATE INDEX chat_message_reports_workspace ON chat_message_reports (workspace_id, created_at);
CREATE INDEX chat_message_reports_conversation ON chat_message_reports (conversation_id);
CREATE INDEX chat_message_reports_reporter ON chat_message_reports (reporter_id);
CREATE INDEX chat_message_reports_author ON chat_message_reports (author_id);

CREATE TABLE user_blocks (
    blocker_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    blocked_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (blocker_id, blocked_id),
    CHECK (blocker_id <> blocked_id)
);

CREATE INDEX user_blocks_blocked ON user_blocks (blocked_id);

-- The kind CHECK changes, so the table is rebuilt as in 0082.
PRAGMA defer_foreign_keys = ON;

CREATE TABLE notifications_backup AS SELECT * FROM notifications;
DROP TABLE notifications;

CREATE TABLE notifications (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    recipient_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    actor_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL
        CHECK (kind IN ('task_assigned', 'comment_mentioned', 'description_mentioned',
                        'page_comment_mentioned', 'page_mentioned', 'chat_mentioned',
                        'import_finished', 'chat_reported')),
    task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    comment_id TEXT REFERENCES task_comments(id) ON DELETE CASCADE,
    page_id TEXT REFERENCES pages(id) ON DELETE CASCADE,
    page_thread_id TEXT REFERENCES page_threads(id) ON DELETE CASCADE,
    page_comment_id TEXT REFERENCES page_comments(id) ON DELETE CASCADE,
    page_block_id TEXT CHECK (page_block_id IS NULL OR length(page_block_id) BETWEEN 1 AND 64),
    chat_conversation_id TEXT REFERENCES chat_conversations(id) ON DELETE CASCADE,
    chat_message_id TEXT REFERENCES chat_messages(id) ON DELETE CASCADE,
    dedupe_key TEXT NOT NULL UNIQUE,
    read_at INTEGER,
    created_at INTEGER NOT NULL,
    pushed_at INTEGER,
    import_source TEXT CHECK (import_source IN ('asana', 'linear', 'trello', 'notion', 'obsidian')),
    import_id TEXT,
    import_status TEXT CHECK (import_status IN ('completed', 'failed')),
    chat_report_id TEXT REFERENCES chat_message_reports(id) ON DELETE CASCADE,
    CHECK ((kind = 'chat_reported') = (chat_report_id IS NOT NULL)),
    CHECK ((kind = 'chat_mentioned') = (chat_conversation_id IS NOT NULL)),
    CHECK ((kind = 'import_finished') = (import_id IS NOT NULL)),
    CHECK ((import_id IS NULL) = (import_source IS NULL) AND (import_id IS NULL) = (import_status IS NULL)),
    CHECK ((chat_conversation_id IS NULL) = (chat_message_id IS NULL)),
    CHECK (
        (kind IN ('task_assigned', 'comment_mentioned', 'description_mentioned')
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
        OR (kind IN ('chat_mentioned', 'import_finished', 'chat_reported')
            AND task_id IS NULL AND comment_id IS NULL
            AND page_id IS NULL AND page_thread_id IS NULL AND page_comment_id IS NULL
            AND page_block_id IS NULL)
    )
);

INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, task_id,
                           comment_id, page_id, page_thread_id, page_comment_id, page_block_id,
                           chat_conversation_id, chat_message_id, dedupe_key, read_at, created_at,
                           pushed_at, import_source, import_id, import_status)
SELECT id, workspace_id, recipient_user_id, actor_user_id, kind, task_id,
       comment_id, page_id, page_thread_id, page_comment_id, page_block_id,
       chat_conversation_id, chat_message_id, dedupe_key, read_at, created_at, pushed_at,
       import_source, import_id, import_status
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
CREATE INDEX notifications_chat_message ON notifications (chat_message_id);
-- Reading a conversation marks its unread mention notifications as read.
CREATE INDEX notifications_chat_unread ON notifications (chat_conversation_id, recipient_user_id)
    WHERE read_at IS NULL AND chat_conversation_id IS NOT NULL;
-- The 10-minute dedupe looks up the recipient's latest page mention of a page.
CREATE INDEX notifications_page_mentions ON notifications (page_id, recipient_user_id, created_at)
    WHERE kind = 'page_mentioned';

CREATE INDEX notifications_unpushed ON notifications (id) WHERE pushed_at IS NULL;
CREATE INDEX notifications_chat_report ON notifications (chat_report_id);

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
