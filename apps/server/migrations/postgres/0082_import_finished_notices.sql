-- When a long import (Asana, Linear, Trello, Notion, Obsidian) ends, its user gets an email and an
-- inbox notification (kind `import_finished`, naming the import). `started_at`: when the user
-- started the import, to tell a long import from a short one.
ALTER TABLE asana_imports ADD COLUMN started_at BIGINT;
ALTER TABLE linear_imports ADD COLUMN started_at BIGINT;
ALTER TABLE trello_imports ADD COLUMN started_at BIGINT;
ALTER TABLE notion_imports ADD COLUMN started_at BIGINT;
ALTER TABLE obsidian_imports ADD COLUMN started_at BIGINT;

ALTER TABLE notifications ADD COLUMN import_source TEXT
    CHECK (import_source IN ('asana', 'linear', 'trello', 'notion', 'obsidian'));
ALTER TABLE notifications ADD COLUMN import_id TEXT;
ALTER TABLE notifications ADD COLUMN import_status TEXT
    CHECK (import_status IN ('completed', 'failed'));

ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications DROP CONSTRAINT notifications_scope_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('task_assigned', 'comment_mentioned', 'description_mentioned',
                    'page_comment_mentioned', 'page_mentioned', 'chat_mentioned',
                    'import_finished'));
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
        OR (kind IN ('chat_mentioned', 'import_finished')
            AND task_id IS NULL AND comment_id IS NULL
            AND page_id IS NULL AND page_thread_id IS NULL AND page_comment_id IS NULL
            AND page_block_id IS NULL)
    );
ALTER TABLE notifications ADD CONSTRAINT notifications_import_check
    CHECK ((kind = 'import_finished') = (import_id IS NOT NULL));
ALTER TABLE notifications ADD CONSTRAINT notifications_import_fields_check
    CHECK ((import_id IS NULL) = (import_source IS NULL) AND (import_id IS NULL) = (import_status IS NULL));
