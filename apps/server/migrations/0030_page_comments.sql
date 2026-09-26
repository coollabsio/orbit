-- Docs comments: threads anchored to text in a page (the anchor is BlockNote's `comment` mark
-- inside the collaborative document; its `threadId` is page_threads.id), their comments with
-- structured @mentions, and page-comment mention notifications in the shared inbox.
-- Access is the page's: teamspace pages for every member, private pages for their owner only.
-- Everything goes with the page (trash purge, teamspace delete, owner delete) and the workspace.
CREATE TABLE page_threads (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    -- The selected text when the thread was started (shown in the Comments panel).
    quote TEXT NOT NULL DEFAULT '' CHECK (length(quote) <= 1000),
    resolved_at INTEGER,
    resolved_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK ((resolved_at IS NULL) OR (resolved_at >= created_at))
);

CREATE INDEX page_threads_page_created ON page_threads (page_id, created_at, id);
CREATE INDEX page_threads_workspace ON page_threads (workspace_id);
CREATE INDEX page_threads_created_by ON page_threads (created_by);
CREATE INDEX page_threads_resolved_by ON page_threads (resolved_by);

CREATE TRIGGER page_threads_scope_insert
BEFORE INSERT ON page_threads
WHEN NOT EXISTS (
    SELECT 1 FROM pages WHERE pages.id = NEW.page_id AND pages.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'thread page must belong to the workspace');
END;

CREATE TRIGGER page_threads_scope_immutable
BEFORE UPDATE OF workspace_id, page_id ON page_threads
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.page_id <> OLD.page_id
BEGIN
    SELECT RAISE(ABORT, 'thread scope is immutable');
END;

CREATE TABLE page_comments (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES page_threads(id) ON DELETE CASCADE,
    author_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    -- BlockNote blocks of the comment editor (paragraphs, styled text, mentions); '[]' once deleted.
    body_json TEXT NOT NULL DEFAULT '[]',
    body_text TEXT NOT NULL DEFAULT '',
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    edited_at INTEGER,
    deleted_at INTEGER
);

CREATE INDEX page_comments_thread_created ON page_comments (thread_id, created_at, id);
CREATE INDEX page_comments_page ON page_comments (page_id);
CREATE INDEX page_comments_workspace ON page_comments (workspace_id);
CREATE INDEX page_comments_author ON page_comments (author_id);

CREATE TRIGGER page_comments_scope_insert
BEFORE INSERT ON page_comments
WHEN NOT EXISTS (
    SELECT 1 FROM page_threads
    WHERE page_threads.id = NEW.thread_id
      AND page_threads.page_id = NEW.page_id
      AND page_threads.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'comment must belong to the page thread');
END;

CREATE TRIGGER page_comments_scope_immutable
BEFORE UPDATE OF workspace_id, page_id, thread_id, author_id ON page_comments
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.page_id <> OLD.page_id
    OR NEW.thread_id <> OLD.thread_id OR NEW.author_id <> OLD.author_id
BEGIN
    SELECT RAISE(ABORT, 'comment scope is immutable');
END;

-- The members a comment mentions (its current body). Notifications go to newly added ones.
CREATE TABLE page_comment_mentions (
    comment_id TEXT NOT NULL REFERENCES page_comments(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (comment_id, user_id)
) WITHOUT ROWID;

CREATE INDEX page_comment_mentions_user ON page_comment_mentions (user_id);

-- Notifications gain page-comment mentions: task_id becomes optional, so the table is rebuilt
-- (SQLite cannot relax NOT NULL / CHECK). The migration runner holds a transaction with
-- foreign_keys=ON; nothing references notifications, so the implicit DELETE of DROP TABLE cascades
-- nowhere. Rows are copied aside and back; the index and trigger (0012) are recreated below.
PRAGMA defer_foreign_keys = ON;

CREATE TABLE notifications_backup AS SELECT * FROM notifications;
DROP TABLE notifications;

CREATE TABLE notifications (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    recipient_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    actor_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL
        CHECK (kind IN ('task_assigned', 'comment_mentioned', 'page_comment_mentioned')),
    task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    comment_id TEXT REFERENCES task_comments(id) ON DELETE CASCADE,
    page_id TEXT REFERENCES pages(id) ON DELETE CASCADE,
    page_thread_id TEXT REFERENCES page_threads(id) ON DELETE CASCADE,
    page_comment_id TEXT REFERENCES page_comments(id) ON DELETE CASCADE,
    dedupe_key TEXT NOT NULL UNIQUE,
    read_at INTEGER,
    created_at INTEGER NOT NULL,
    CHECK (
        (kind IN ('task_assigned', 'comment_mentioned')
            AND task_id IS NOT NULL
            AND page_id IS NULL AND page_thread_id IS NULL AND page_comment_id IS NULL)
        OR (kind = 'page_comment_mentioned'
            AND task_id IS NULL AND comment_id IS NULL
            AND page_id IS NOT NULL AND page_thread_id IS NOT NULL AND page_comment_id IS NOT NULL)
    )
);

INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, task_id,
                           comment_id, dedupe_key, read_at, created_at)
SELECT id, workspace_id, recipient_user_id, actor_user_id, kind, task_id,
       comment_id, dedupe_key, read_at, created_at
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
