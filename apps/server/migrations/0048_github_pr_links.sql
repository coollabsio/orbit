-- Pull requests that name a task (in the branch, the title, or after a keyword in the body),
-- and the per-project rules that move a task when such a pull request moves.

-- Nothing wrote to the old table (only a DELETE used it), so it is recreated with the new
-- columns. One row for each (pull request, task).
DROP TABLE github_pull_links;

CREATE TABLE github_pull_links (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    repository TEXT NOT NULL,
    pull_number INTEGER NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    url TEXT NOT NULL,
    branch TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('draft', 'open', 'in_review', 'merged', 'closed')),
    -- The reference closes the task (branch, title, or "Fixes"); "Refs" does not.
    closes INTEGER NOT NULL CHECK (closes IN (0, 1)),
    -- The author is an owner, member or collaborator of the repository. Only such a pull
    -- request changes a task status.
    trusted INTEGER NOT NULL CHECK (trusted IN (0, 1)),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, repository, pull_number, task_id)
);

CREATE INDEX github_pull_links_task ON github_pull_links (task_id);

-- The status a task of the project takes on a pull request event (`draft`, `open`, `review`,
-- `merged`; text, so a new event needs no migration). No row: the default of the event. A row
-- with a NULL status: no change. A deleted status takes its row along, back to the default.
CREATE TABLE project_pr_automation (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    event TEXT NOT NULL CHECK (length(event) BETWEEN 1 AND 32),
    status_id TEXT REFERENCES task_statuses(id) ON DELETE CASCADE,
    PRIMARY KEY (project_id, event)
);

-- For the cascade when a status is deleted.
CREATE INDEX project_pr_automation_status ON project_pr_automation (status_id);
