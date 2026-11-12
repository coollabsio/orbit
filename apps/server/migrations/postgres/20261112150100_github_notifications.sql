-- GitHub pull request notices, as in migrations/20261112150100_github_notifications.sql.
ALTER TABLE notification_prefs ADD COLUMN github BIGINT NOT NULL DEFAULT 1 CHECK (github IN (0, 1));

ALTER TABLE notifications ADD COLUMN github_repository TEXT;
ALTER TABLE notifications ADD COLUMN github_pull_number BIGINT;
ALTER TABLE notifications ADD COLUMN github_actor_login TEXT;
ALTER TABLE notifications ADD COLUMN github_review_state TEXT
    CHECK (github_review_state IN ('approved', 'changes_requested', 'commented'));
ALTER TABLE notifications ADD COLUMN github_comment_id BIGINT;

ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications DROP CONSTRAINT notifications_scope_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('task_assigned', 'comment_mentioned', 'description_mentioned',
                    'page_comment_mentioned', 'page_mentioned', 'chat_mentioned',
                    'import_finished', 'github_review_requested', 'github_review',
                    'github_merged'));
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
        OR (kind IN ('chat_mentioned', 'import_finished', 'github_review_requested', 'github_review', 'github_merged')
            AND task_id IS NULL AND comment_id IS NULL
            AND page_id IS NULL AND page_thread_id IS NULL AND page_comment_id IS NULL
            AND page_block_id IS NULL)
    );
ALTER TABLE notifications ADD CONSTRAINT notifications_github_check
    CHECK (
        ((kind LIKE 'github_%') = (github_repository IS NOT NULL))
        AND ((github_repository IS NULL) = (github_pull_number IS NULL))
        AND ((github_repository IS NULL) = (github_actor_login IS NULL))
        AND ((kind = 'github_review') = (github_review_state IS NOT NULL))
    );
