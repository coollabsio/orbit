-- The public API import (`tasks:import` tokens): the source tool's ID of an imported task or
-- comment, unique in the workspace, so a restarted import finds what it already wrote instead
-- of writing it twice. NULL for everything else.
ALTER TABLE tasks ADD COLUMN external_id TEXT;
ALTER TABLE task_comments ADD COLUMN external_id TEXT;
CREATE UNIQUE INDEX tasks_external_id ON tasks (workspace_id, external_id);
CREATE UNIQUE INDEX task_comments_external_id ON task_comments (workspace_id, external_id);
