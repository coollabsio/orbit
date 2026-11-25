-- SQLite. Postgres: postgres/20261125062505_time_entry_comments.sql.
--
-- A time entry logged with a comment (`/spend` in the composer, issue #95) names that comment, so the
-- comment shows the duration. The entry keeps its own copy of the note: deleting the comment only
-- clears the link.
ALTER TABLE task_time_entries ADD COLUMN comment_id TEXT REFERENCES task_comments(id) ON DELETE SET NULL;
CREATE INDEX task_time_entries_comment ON task_time_entries (comment_id);
