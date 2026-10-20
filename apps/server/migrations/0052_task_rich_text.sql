-- Rich text and live co-editing for tasks.
--
-- Task descriptions and comments store BlockNote JSON (`description_json`, `body_json`) as the
-- source; `description` and `body` stay their markdown copy (search, MCP, GitHub, API). NULL JSON
-- is an older row that has only markdown. A task description is co-edited live like a page: each
-- task gets its Yjs document in `task_collab_docs` and `task_collab_updates` (same shape and rules
-- as the page tables in 0029; `page_collab_meta.generation` serves both kinds). Files pasted into
-- the text are attachment references with `inline = 1`, which the attachment lists leave out.
ALTER TABLE tasks ADD COLUMN description_json TEXT;
ALTER TABLE task_comments ADD COLUMN body_json TEXT;
ALTER TABLE attachment_references ADD COLUMN inline INTEGER NOT NULL DEFAULT 0 CHECK (inline IN (0, 1));

CREATE TABLE task_collab_docs (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    snapshot BLOB NOT NULL,
    snapshot_seq INTEGER NOT NULL DEFAULT 0 CHECK (snapshot_seq >= 0),
    epoch TEXT NOT NULL CHECK (length(epoch) BETWEEN 1 AND 64),
    converter_version INTEGER NOT NULL,
    projected_seq INTEGER NOT NULL DEFAULT 0 CHECK (projected_seq >= 0),
    audited_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX task_collab_docs_workspace ON task_collab_docs (workspace_id);

CREATE TABLE task_collab_updates (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    data BLOB NOT NULL,
    user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX task_collab_updates_task ON task_collab_updates (task_id, seq);
CREATE INDEX task_collab_updates_workspace ON task_collab_updates (workspace_id);
CREATE INDEX task_collab_updates_user ON task_collab_updates (user_id);

CREATE TRIGGER task_collab_docs_scope_insert
BEFORE INSERT ON task_collab_docs
WHEN NOT EXISTS (
    SELECT 1 FROM tasks WHERE tasks.id = NEW.task_id AND tasks.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'collaborative document must belong to the task workspace');
END;

CREATE TRIGGER task_collab_docs_scope_immutable
BEFORE UPDATE OF workspace_id, task_id ON task_collab_docs
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.task_id <> OLD.task_id
BEGIN
    SELECT RAISE(ABORT, 'collaborative document scope is immutable');
END;

CREATE TRIGGER task_collab_updates_scope_insert
BEFORE INSERT ON task_collab_updates
WHEN NOT EXISTS (
    SELECT 1 FROM tasks WHERE tasks.id = NEW.task_id AND tasks.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'collaborative update must belong to the task workspace');
END;

CREATE TRIGGER task_collab_updates_scope_immutable
BEFORE UPDATE OF workspace_id, task_id ON task_collab_updates
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.task_id <> OLD.task_id
BEGIN
    SELECT RAISE(ABORT, 'collaborative update scope is immutable');
END;

-- A member mentioned in a task description is notified (kind `description_mentioned`, no
-- comment). The kind CHECK changes, so the table is rebuilt as in 0037 (with 0040's pushed_at).
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
                        'page_comment_mentioned', 'page_mentioned', 'chat_mentioned')),
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
    CHECK ((kind = 'chat_mentioned') = (chat_conversation_id IS NOT NULL)),
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
        OR (kind = 'chat_mentioned'
            AND task_id IS NULL AND comment_id IS NULL
            AND page_id IS NULL AND page_thread_id IS NULL AND page_comment_id IS NULL
            AND page_block_id IS NULL)
    )
);

INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, task_id,
                           comment_id, page_id, page_thread_id, page_comment_id, page_block_id,
                           chat_conversation_id, chat_message_id, dedupe_key, read_at, created_at,
                           pushed_at)
SELECT id, workspace_id, recipient_user_id, actor_user_id, kind, task_id,
       comment_id, page_id, page_thread_id, page_comment_id, page_block_id,
       chat_conversation_id, chat_message_id, dedupe_key, read_at, created_at, pushed_at
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
