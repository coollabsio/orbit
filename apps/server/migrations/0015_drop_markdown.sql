-- The rich text data migration registered after schema version 14 has already
-- converted every row. The Markdown source is no longer read by any code path.
ALTER TABLE tasks DROP COLUMN description;
ALTER TABLE task_comments DROP COLUMN body;
