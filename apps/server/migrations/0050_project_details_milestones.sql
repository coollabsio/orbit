-- Project details, milestones and the roadmap (Linear parity, area 4).
--
-- An Orbit project is a product and has no end date. Dates, lifecycle and health belong to the
-- milestone. Every change here is an ADD COLUMN or a new table, so no table is rebuilt and the FK
-- child order the retention purge depends on is unchanged (.ai/lessons.md, "RESTRICT").

-- Project lead and members. All workspace members can still see and edit every project.
ALTER TABLE projects ADD COLUMN lead_user_id TEXT REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE projects ADD COLUMN overview_page_id TEXT REFERENCES pages(id) ON DELETE SET NULL;

CREATE TABLE project_members (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (project_id, user_id),
    UNIQUE (project_id, membership_id)
);

CREATE INDEX project_members_user_project ON project_members (user_id, project_id);

CREATE TRIGGER project_members_validate_scope_insert
BEFORE INSERT ON project_members
WHEN NOT EXISTS (
    SELECT 1 FROM projects JOIN memberships
      ON memberships.id = NEW.membership_id
     AND memberships.workspace_id = projects.workspace_id
     AND memberships.user_id = NEW.user_id
    JOIN users ON users.id = memberships.user_id AND users.suspended_at IS NULL
    WHERE projects.id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'project member must be an active workspace membership');
END;

CREATE TRIGGER project_members_validate_scope_update
BEFORE UPDATE OF project_id, membership_id, user_id ON project_members
WHEN NOT EXISTS (
    SELECT 1 FROM projects JOIN memberships
      ON memberships.id = NEW.membership_id
     AND memberships.workspace_id = projects.workspace_id
     AND memberships.user_id = NEW.user_id
    JOIN users ON users.id = memberships.user_id AND users.suspended_at IS NULL
    WHERE projects.id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'project member must be an active workspace membership');
END;

-- ON DELETE SET NULL fires this trigger with a NULL lead, so it guards on IS NOT NULL.
CREATE TRIGGER projects_lead_validate_update
BEFORE UPDATE OF lead_user_id ON projects
WHEN NEW.lead_user_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM memberships
    JOIN users ON users.id = memberships.user_id AND users.suspended_at IS NULL
    WHERE memberships.workspace_id = NEW.workspace_id AND memberships.user_id = NEW.lead_user_id
)
BEGIN
    SELECT RAISE(ABORT, 'project lead must be an active workspace membership');
END;

CREATE TRIGGER users_remove_project_roles_on_suspend
AFTER UPDATE OF suspended_at ON users
WHEN OLD.suspended_at IS NULL AND NEW.suspended_at IS NOT NULL
BEGIN
    DELETE FROM project_members WHERE user_id = NEW.id;
    UPDATE projects SET lead_user_id = NULL WHERE lead_user_id = NEW.id;
END;

-- project_members rows go with the membership (FK cascade); the lead has no membership column.
CREATE TRIGGER memberships_clear_project_lead_on_delete
AFTER DELETE ON memberships
BEGIN
    UPDATE projects SET lead_user_id = NULL
    WHERE workspace_id = OLD.workspace_id AND lead_user_id = OLD.user_id;
END;

-- Owned pages: the description of a project or a milestone is a real Docs page that Docs hides.
-- An owned page has no space (no teamspace and no private owner), so every Docs list, which
-- filters on the space, never returns it. Access is the access of the project.
ALTER TABLE pages ADD COLUMN owner_kind TEXT;

DROP TRIGGER pages_space_exactly_one_insert;
DROP TRIGGER pages_space_exactly_one_update;

CREATE TRIGGER pages_space_exactly_one_insert
BEFORE INSERT ON pages
WHEN (NEW.owner_kind IS NULL AND (NEW.teamspace_id IS NULL) = (NEW.owner_id IS NULL))
  OR (NEW.owner_kind IS NOT NULL
      AND (NEW.teamspace_id IS NOT NULL OR NEW.owner_id IS NOT NULL OR NEW.parent_id IS NOT NULL))
BEGIN
    SELECT RAISE(ABORT, 'page must belong to exactly one space');
END;

CREATE TRIGGER pages_space_exactly_one_update
BEFORE UPDATE OF teamspace_id, owner_id, owner_kind, parent_id ON pages
WHEN (NEW.owner_kind IS NULL AND (NEW.teamspace_id IS NULL) = (NEW.owner_id IS NULL))
  OR (NEW.owner_kind IS NOT NULL
      AND (NEW.teamspace_id IS NOT NULL OR NEW.owner_id IS NOT NULL OR NEW.parent_id IS NOT NULL))
BEGIN
    SELECT RAISE(ABORT, 'page must belong to exactly one space');
END;

-- Milestones. `status` (planned | in_progress | completed | cancelled) is checked in server code,
-- so a new value needs no table rebuild. `start_at` and `target_at` are days, stored as the
-- millisecond timestamp of the day.
CREATE TABLE milestones (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    status TEXT NOT NULL,
    start_at INTEGER,
    target_at INTEGER,
    description_page_id TEXT REFERENCES pages(id) ON DELETE SET NULL,
    position INTEGER NOT NULL,
    completed_at INTEGER,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (start_at IS NULL OR target_at IS NULL OR target_at >= start_at)
);

CREATE INDEX milestones_project_position ON milestones (project_id, position, id);
CREATE INDEX milestones_workspace ON milestones (workspace_id, project_id);

CREATE TRIGGER milestones_validate_scope_insert
BEFORE INSERT ON milestones
WHEN NOT EXISTS (
    SELECT 1 FROM projects
    WHERE projects.id = NEW.project_id AND projects.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'milestone project must belong to the workspace');
END;

CREATE TRIGGER milestones_scope_immutable
BEFORE UPDATE OF workspace_id, project_id ON milestones
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.project_id <> OLD.project_id
BEGIN
    SELECT RAISE(ABORT, 'milestone scope is immutable');
END;

CREATE TABLE milestone_updates (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    milestone_id TEXT NOT NULL REFERENCES milestones(id) ON DELETE CASCADE,
    author_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    health TEXT NOT NULL,
    body TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX milestone_updates_milestone_created
    ON milestone_updates (milestone_id, created_at DESC, id DESC);

-- A task's milestone must be in the task's project. ON DELETE SET NULL (milestone delete, project
-- purge) fires the UPDATE trigger with a NULL milestone, so both triggers guard on IS NOT NULL.
ALTER TABLE tasks ADD COLUMN milestone_id TEXT REFERENCES milestones(id) ON DELETE SET NULL;

CREATE INDEX tasks_milestone_idx ON tasks (milestone_id) WHERE milestone_id IS NOT NULL;

CREATE TRIGGER tasks_milestone_validate_insert
BEFORE INSERT ON tasks
WHEN NEW.milestone_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM milestones
    WHERE milestones.id = NEW.milestone_id AND milestones.project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'task milestone must belong to the task project');
END;

CREATE TRIGGER tasks_milestone_validate_update
BEFORE UPDATE OF project_id, milestone_id ON tasks
WHEN NEW.milestone_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM milestones
    WHERE milestones.id = NEW.milestone_id AND milestones.project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'task milestone must belong to the task project');
END;

-- The owner's hard delete (retention purge of a project, delete of a milestone, also through the
-- project cascade) deletes the owned page.
CREATE TRIGGER projects_delete_overview_page
AFTER DELETE ON projects
WHEN OLD.overview_page_id IS NOT NULL
BEGIN
    DELETE FROM pages WHERE id = OLD.overview_page_id AND owner_kind = 'project';
END;

CREATE TRIGGER milestones_delete_description_page
AFTER DELETE ON milestones
WHEN OLD.description_page_id IS NOT NULL
BEGIN
    DELETE FROM pages WHERE id = OLD.description_page_id AND owner_kind = 'milestone';
END;

-- A health update of a milestone notifies the project lead and members: the inbox row points to
-- the milestone. `notifications.kind` is free text and the target CHECK allows a row with no
-- task, page or chat target (0045), so this is an ADD COLUMN; the scope trigger gets the
-- milestone check.
ALTER TABLE notifications ADD COLUMN milestone_id TEXT REFERENCES milestones(id) ON DELETE CASCADE;

CREATE INDEX notifications_milestone ON notifications (milestone_id) WHERE milestone_id IS NOT NULL;

DROP TRIGGER notifications_validate_scope_insert;

CREATE TRIGGER notifications_validate_scope_insert
BEFORE INSERT ON notifications
WHEN NOT EXISTS (
    SELECT 1 FROM memberships
    WHERE memberships.workspace_id = NEW.workspace_id
      AND memberships.user_id = NEW.recipient_user_id
) OR (
    NEW.task_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM tasks
        WHERE tasks.id = NEW.task_id AND tasks.workspace_id = NEW.workspace_id
    )
) OR (
    NEW.comment_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM task_comments
        WHERE task_comments.id = NEW.comment_id
          AND task_comments.task_id = NEW.task_id
          AND task_comments.workspace_id = NEW.workspace_id
    )
) OR (
    NEW.page_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM pages
        WHERE pages.id = NEW.page_id
          AND pages.workspace_id = NEW.workspace_id
          AND (pages.owner_id IS NULL OR pages.owner_id = NEW.recipient_user_id)
    )
) OR (
    NEW.chat_message_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM chat_messages
        JOIN chat_conversations ON chat_conversations.id = chat_messages.conversation_id
        WHERE chat_messages.id = NEW.chat_message_id
          AND chat_messages.conversation_id = NEW.chat_conversation_id
          AND chat_conversations.workspace_id = NEW.workspace_id
    )
) OR (
    NEW.milestone_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM milestones
        WHERE milestones.id = NEW.milestone_id AND milestones.workspace_id = NEW.workspace_id
    )
) OR (
    NEW.page_comment_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM page_comments
        WHERE page_comments.id = NEW.page_comment_id
          AND page_comments.thread_id = NEW.page_thread_id
          AND page_comments.page_id = NEW.page_id
          AND page_comments.workspace_id = NEW.workspace_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'notification must belong to the workspace task, a page the recipient can see or a chat message');
END;
