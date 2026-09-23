CREATE TABLE github_issue_links (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    repository TEXT NOT NULL,
    issue_number INTEGER NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    sync_paused INTEGER NOT NULL DEFAULT 0 CHECK (sync_paused IN (0, 1)),
    kind TEXT NOT NULL DEFAULT 'issue' CHECK (kind IN ('issue', 'pull_request')),
    pull_state TEXT NOT NULL DEFAULT 'open' CHECK (pull_state IN ('open', 'closed', 'merged')),
    PRIMARY KEY (workspace_id, repository, issue_number),
    UNIQUE (task_id)
);

CREATE TABLE github_pull_links (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    repository TEXT NOT NULL,
    pull_number INTEGER NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    url TEXT NOT NULL,
    state TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, repository, pull_number, task_id)
);

CREATE INDEX github_pull_links_task ON github_pull_links (task_id);

CREATE TABLE github_apps (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    app_id INTEGER NOT NULL UNIQUE,
    slug TEXT NOT NULL,
    private_key_encrypted BLOB NOT NULL,
    webhook_secret_encrypted BLOB NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE github_app_registrations (
    state_hash BLOB PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL,
    public_origin TEXT NOT NULL DEFAULT '',
    UNIQUE (workspace_id)
);

CREATE TABLE github_installations (
    installation_id INTEGER PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES github_apps(workspace_id) ON DELETE CASCADE,
    account_login TEXT NOT NULL
);

CREATE TABLE github_installation_repositories (
    installation_id INTEGER NOT NULL REFERENCES github_installations(installation_id) ON DELETE CASCADE,
    repository TEXT NOT NULL,
    PRIMARY KEY (installation_id, repository)
);

CREATE TABLE github_project_connections (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    installation_id INTEGER NOT NULL REFERENCES github_installations(installation_id) ON DELETE CASCADE,
    repository TEXT NOT NULL,
    label TEXT NOT NULL,
    UNIQUE (workspace_id, repository, label)
);

ALTER TABLE tasks ADD COLUMN source_url TEXT;

-- Discord used the same trailing source line. The message URL contains no whitespace.
UPDATE tasks
SET source_url = substr(description, instr(description, char(10) || char(10) || 'Source: https://discord.com/channels/') + length(char(10) || char(10) || 'Source: ')),
    description = substr(description, 1, instr(description, char(10) || char(10) || 'Source: https://discord.com/channels/') - 1)
WHERE id IN (SELECT task_id FROM integration_events WHERE provider = 'discord')
  AND instr(description, char(10) || char(10) || 'Source: https://discord.com/channels/') > 0;
