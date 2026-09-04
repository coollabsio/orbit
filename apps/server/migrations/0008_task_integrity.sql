CREATE TRIGGER projects_workspace_immutable
BEFORE UPDATE OF workspace_id ON projects
WHEN NEW.workspace_id <> OLD.workspace_id
BEGIN
    SELECT RAISE(ABORT, 'project workspace is immutable');
END;

CREATE TRIGGER task_statuses_scope_immutable
BEFORE UPDATE OF workspace_id, project_id ON task_statuses
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.project_id <> OLD.project_id
BEGIN
    SELECT RAISE(ABORT, 'task status scope is immutable');
END;

CREATE TRIGGER tasks_workspace_immutable
BEFORE UPDATE OF workspace_id ON tasks
WHEN NEW.workspace_id <> OLD.workspace_id
BEGIN
    SELECT RAISE(ABORT, 'task workspace is immutable');
END;

CREATE TRIGGER labels_workspace_immutable
BEFORE UPDATE OF workspace_id ON labels
WHEN NEW.workspace_id <> OLD.workspace_id
BEGIN
    SELECT RAISE(ABORT, 'label workspace is immutable');
END;

CREATE TRIGGER memberships_identity_immutable
BEFORE UPDATE OF workspace_id, user_id ON memberships
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.user_id <> OLD.user_id
BEGIN
    SELECT RAISE(ABORT, 'membership identity is immutable');
END;

CREATE TRIGGER task_comments_scope_immutable
BEFORE UPDATE OF workspace_id, task_id ON task_comments
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.task_id <> OLD.task_id
BEGIN
    SELECT RAISE(ABORT, 'task comment scope is immutable');
END;

CREATE TRIGGER users_remove_task_assignments_on_suspend
AFTER UPDATE OF suspended_at ON users
WHEN OLD.suspended_at IS NULL AND NEW.suspended_at IS NOT NULL
BEGIN
    DELETE FROM task_assignees WHERE user_id = NEW.id;
END;
