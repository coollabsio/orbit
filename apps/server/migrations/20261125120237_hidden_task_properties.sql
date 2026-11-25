-- SQLite
-- The built-in task properties the workspace hides in its task UI, a JSON array of
-- "milestone", "cycle" and "estimate". Empty: all show. Hiding keeps the tasks' values.
ALTER TABLE workspaces ADD COLUMN hidden_task_properties TEXT NOT NULL DEFAULT '[]';
