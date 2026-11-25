-- Postgres. SQLite: ../20261124133110_task_time_entries.sql.
--
-- Time logged on a task (issue #95): who worked, how many minutes, on which calendar day, and a
-- Markdown note. `worked_on` is a `YYYY-MM-DD` date in the person's timezone, not a timestamp, so an
-- entry stays on the same day in every report.
CREATE TABLE task_time_entries (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    minutes BIGINT NOT NULL CHECK (minutes BETWEEN 1 AND 1440),
    worked_on TEXT NOT NULL CHECK (length(worked_on) = 10),
    note TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 10000),
    billable BIGINT NOT NULL DEFAULT 0 CHECK (billable IN (0, 1)),
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    version BIGINT NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX task_time_entries_task ON task_time_entries (task_id, worked_on, created_at);
CREATE INDEX task_time_entries_user ON task_time_entries (workspace_id, user_id, worked_on);
