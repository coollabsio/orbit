-- Postgres. SQLite: ../20261118130637_chat_reports_and_blocks.sql.
--
-- App Store guideline 1.2 (user content): chat message reports and user blocks. See the SQLite
-- file for the meaning.
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
    created_at BIGINT NOT NULL,
    UNIQUE (message_id, reporter_id)
);

CREATE INDEX chat_message_reports_workspace ON chat_message_reports (workspace_id, created_at);
CREATE INDEX chat_message_reports_conversation ON chat_message_reports (conversation_id);
CREATE INDEX chat_message_reports_reporter ON chat_message_reports (reporter_id);
CREATE INDEX chat_message_reports_author ON chat_message_reports (author_id);

CREATE TABLE user_blocks (
    blocker_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    blocked_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (blocker_id, blocked_id),
    CHECK (blocker_id <> blocked_id)
);

CREATE INDEX user_blocks_blocked ON user_blocks (blocked_id);

ALTER TABLE notifications ADD COLUMN chat_report_id TEXT
    REFERENCES chat_message_reports(id) ON DELETE CASCADE;
CREATE INDEX notifications_chat_report ON notifications (chat_report_id);

ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications DROP CONSTRAINT notifications_scope_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('task_assigned', 'comment_mentioned', 'description_mentioned',
                    'page_comment_mentioned', 'page_mentioned', 'chat_mentioned',
                    'import_finished', 'chat_reported'));
ALTER TABLE notifications ADD CONSTRAINT notifications_scope_check
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
    );
ALTER TABLE notifications ADD CONSTRAINT notifications_report_check
    CHECK ((kind = 'chat_reported') = (chat_report_id IS NOT NULL));
