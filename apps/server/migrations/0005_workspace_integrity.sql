ALTER TABLE workspaces ADD COLUMN owner_membership_id TEXT
    REFERENCES memberships(id) DEFERRABLE INITIALLY DEFERRED;

UPDATE workspaces
SET owner_membership_id = (
    SELECT memberships.id
    FROM memberships
    WHERE memberships.workspace_id = workspaces.id
      AND memberships.role = 'owner'
);

CREATE UNIQUE INDEX workspaces_owner_membership
    ON workspaces (owner_membership_id);

CREATE TRIGGER workspaces_require_owner_insert
BEFORE INSERT ON workspaces
WHEN NEW.owner_membership_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'workspace owner membership is required');
END;

CREATE TRIGGER workspaces_require_owner_update
BEFORE UPDATE OF owner_membership_id ON workspaces
WHEN NEW.owner_membership_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'workspace owner membership is required');
END;

CREATE TRIGGER workspaces_validate_owner_update
BEFORE UPDATE OF owner_membership_id ON workspaces
WHEN NOT EXISTS (
    SELECT 1 FROM memberships
    WHERE memberships.id = NEW.owner_membership_id
      AND memberships.workspace_id = NEW.id
)
BEGIN
    SELECT RAISE(ABORT, 'workspace owner membership must belong to the workspace');
END;

CREATE TRIGGER workspaces_apply_owner_update
AFTER UPDATE OF owner_membership_id ON workspaces
WHEN OLD.owner_membership_id != NEW.owner_membership_id
BEGIN
    UPDATE memberships
    SET role = 'admin', version = version + 1, updated_at = NEW.updated_at
    WHERE id = OLD.owner_membership_id AND workspace_id = NEW.id;
    UPDATE memberships
    SET role = 'owner', version = version + 1, updated_at = NEW.updated_at
    WHERE id = NEW.owner_membership_id AND workspace_id = NEW.id;
END;

CREATE TRIGGER memberships_protect_current_owner_role
BEFORE UPDATE OF role ON memberships
WHEN OLD.role = 'owner' AND NEW.role != 'owner'
 AND EXISTS (
    SELECT 1 FROM workspaces
    WHERE workspaces.id = OLD.workspace_id
      AND workspaces.owner_membership_id = OLD.id
 )
BEGIN
    SELECT RAISE(ABORT, 'workspace owner role is protected');
END;

CREATE TRIGGER memberships_restrict_owner_role
BEFORE UPDATE OF role ON memberships
WHEN NEW.role = 'owner'
 AND NOT EXISTS (
    SELECT 1 FROM workspaces
    WHERE workspaces.id = NEW.workspace_id
      AND workspaces.owner_membership_id = NEW.id
 )
BEGIN
    SELECT RAISE(ABORT, 'workspace owner role must match owner membership');
END;

CREATE TABLE maintenance_summaries (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    workspaces_purged INTEGER NOT NULL DEFAULT 0 CHECK (workspaces_purged >= 0),
    audit_events_purged INTEGER NOT NULL DEFAULT 0 CHECK (audit_events_purged >= 0),
    occurred_at INTEGER NOT NULL
);

CREATE INDEX maintenance_summaries_kind_time
    ON maintenance_summaries (kind, occurred_at);
