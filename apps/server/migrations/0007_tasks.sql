ALTER TABLE projects ADD COLUMN restore_project_key TEXT;

CREATE TABLE labels (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    color TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (workspace_id, name)
);

CREATE INDEX labels_workspace_name ON labels (workspace_id, name, id);
CREATE INDEX projects_workspace_name ON projects (workspace_id, name, id);
CREATE INDEX task_statuses_project_position ON task_statuses (workspace_id, project_id, position, id);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    status_id TEXT NOT NULL REFERENCES task_statuses(id) ON DELETE RESTRICT,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    priority TEXT NOT NULL DEFAULT 'none'
        CHECK (priority IN ('none', 'low', 'medium', 'high', 'urgent')),
    position INTEGER NOT NULL DEFAULT 0,
    creator_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    deleted_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX tasks_workspace_position ON tasks (workspace_id, position, id);
CREATE INDEX tasks_workspace_title ON tasks (workspace_id, title, id);
CREATE INDEX tasks_workspace_created ON tasks (workspace_id, created_at, id);
CREATE INDEX tasks_workspace_updated ON tasks (workspace_id, updated_at, id);
CREATE INDEX tasks_project_status ON tasks (workspace_id, project_id, status_id, id);
CREATE INDEX tasks_workspace_deleted ON tasks (workspace_id, deleted_at, id);

CREATE TABLE task_assignees (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    PRIMARY KEY (task_id, user_id),
    UNIQUE (task_id, membership_id)
);

CREATE INDEX task_assignees_user_task ON task_assignees (user_id, task_id);

CREATE TABLE task_labels (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    label_id TEXT NOT NULL REFERENCES labels(id) ON DELETE RESTRICT,
    PRIMARY KEY (task_id, label_id)
);

CREATE INDEX task_labels_label_task ON task_labels (label_id, task_id);

CREATE TABLE task_comments (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    author_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    parent_id TEXT REFERENCES task_comments(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX task_comments_task_created ON task_comments (task_id, created_at, id);

CREATE TRIGGER task_statuses_validate_scope_insert
BEFORE INSERT ON task_statuses
WHEN NOT EXISTS (
    SELECT 1 FROM projects
    WHERE projects.id = NEW.project_id AND projects.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task status project must belong to the workspace');
END;

CREATE TRIGGER task_statuses_validate_scope_update
BEFORE UPDATE OF workspace_id, project_id ON task_statuses
WHEN NOT EXISTS (
    SELECT 1 FROM projects
    WHERE projects.id = NEW.project_id AND projects.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task status project must belong to the workspace');
END;

CREATE TRIGGER tasks_validate_scope_insert
BEFORE INSERT ON tasks
WHEN NOT EXISTS (
    SELECT 1 FROM task_statuses
    WHERE task_statuses.id = NEW.status_id
      AND task_statuses.project_id = NEW.project_id
      AND task_statuses.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task project and status must belong to the workspace');
END;

CREATE TRIGGER tasks_validate_scope_update
BEFORE UPDATE OF workspace_id, project_id, status_id ON tasks
WHEN NOT EXISTS (
    SELECT 1 FROM task_statuses
    WHERE task_statuses.id = NEW.status_id
      AND task_statuses.project_id = NEW.project_id
      AND task_statuses.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task project and status must belong to the workspace');
END;

CREATE TRIGGER task_assignees_validate_scope_insert
BEFORE INSERT ON task_assignees
WHEN NOT EXISTS (
    SELECT 1 FROM tasks JOIN memberships
      ON memberships.id = NEW.membership_id
     AND memberships.workspace_id = tasks.workspace_id
     AND memberships.user_id = NEW.user_id
    JOIN users ON users.id = memberships.user_id AND users.suspended_at IS NULL
    WHERE tasks.id = NEW.task_id
)
BEGIN
    SELECT RAISE(ABORT, 'task assignee must be an active workspace membership');
END;

CREATE TRIGGER task_assignees_validate_scope_update
BEFORE UPDATE OF task_id, membership_id, user_id ON task_assignees
WHEN NOT EXISTS (
    SELECT 1 FROM tasks JOIN memberships
      ON memberships.id = NEW.membership_id
     AND memberships.workspace_id = tasks.workspace_id
     AND memberships.user_id = NEW.user_id
    JOIN users ON users.id = memberships.user_id AND users.suspended_at IS NULL
    WHERE tasks.id = NEW.task_id
)
BEGIN
    SELECT RAISE(ABORT, 'task assignee must be an active workspace membership');
END;

CREATE TRIGGER task_labels_validate_scope_insert
BEFORE INSERT ON task_labels
WHEN NOT EXISTS (
    SELECT 1 FROM tasks JOIN labels ON labels.id = NEW.label_id
    WHERE tasks.id = NEW.task_id AND labels.workspace_id = tasks.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task label must belong to the workspace');
END;

CREATE TRIGGER task_labels_validate_scope_update
BEFORE UPDATE OF task_id, label_id ON task_labels
WHEN NOT EXISTS (
    SELECT 1 FROM tasks JOIN labels ON labels.id = NEW.label_id
    WHERE tasks.id = NEW.task_id AND labels.workspace_id = tasks.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'task label must belong to the workspace');
END;

CREATE TRIGGER task_comments_validate_scope_insert
BEFORE INSERT ON task_comments
WHEN NOT EXISTS (
    SELECT 1 FROM tasks
    WHERE tasks.id = NEW.task_id AND tasks.workspace_id = NEW.workspace_id
) OR (
    NEW.parent_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM task_comments
        WHERE task_comments.id = NEW.parent_id
          AND task_comments.task_id = NEW.task_id
          AND task_comments.workspace_id = NEW.workspace_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'task comment must belong to its task and workspace');
END;

CREATE TRIGGER task_comments_validate_scope_update
BEFORE UPDATE OF workspace_id, task_id, parent_id ON task_comments
WHEN NOT EXISTS (
    SELECT 1 FROM tasks
    WHERE tasks.id = NEW.task_id AND tasks.workspace_id = NEW.workspace_id
) OR (
    NEW.parent_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM task_comments
        WHERE task_comments.id = NEW.parent_id
          AND task_comments.task_id = NEW.task_id
          AND task_comments.workspace_id = NEW.workspace_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'task comment must belong to its task and workspace');
END;
