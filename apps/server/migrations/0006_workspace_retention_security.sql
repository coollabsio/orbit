CREATE TRIGGER workspaces_validate_owner_insert
BEFORE INSERT ON workspaces
WHEN EXISTS (
    SELECT 1 FROM memberships
    WHERE memberships.id = NEW.owner_membership_id
      AND (memberships.workspace_id != NEW.id OR memberships.role != 'owner')
)
BEGIN
    SELECT RAISE(ABORT, 'workspace owner membership must belong to the workspace');
END;

CREATE TRIGGER memberships_validate_owner_insert
BEFORE INSERT ON memberships
WHEN (
    NEW.role = 'owner' AND NOT EXISTS (
        SELECT 1 FROM workspaces
        WHERE workspaces.id = NEW.workspace_id
          AND workspaces.owner_membership_id = NEW.id
    )
) OR EXISTS (
    SELECT 1 FROM workspaces
    WHERE workspaces.owner_membership_id = NEW.id
      AND (workspaces.id != NEW.workspace_id OR NEW.role != 'owner')
)
BEGIN
    SELECT RAISE(ABORT, 'workspace owner role must match owner membership');
END;

CREATE TABLE attachment_file_deletions (
    id TEXT PRIMARY KEY,
    path_kind TEXT NOT NULL CHECK (path_kind IN ('blob', 'temporary')),
    path TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (path_kind, path)
);

CREATE TABLE security_probe_summaries (
    kind TEXT NOT NULL,
    bucket_started_at INTEGER NOT NULL,
    attempt_count INTEGER NOT NULL CHECK (attempt_count > 0),
    last_attempt_at INTEGER NOT NULL,
    PRIMARY KEY (kind, bucket_started_at)
);

ALTER TABLE maintenance_summaries ADD COLUMN attachment_references_purged INTEGER NOT NULL DEFAULT 0
    CHECK (attachment_references_purged >= 0);
ALTER TABLE maintenance_summaries ADD COLUMN attachment_blobs_purged INTEGER NOT NULL DEFAULT 0
    CHECK (attachment_blobs_purged >= 0);
ALTER TABLE maintenance_summaries ADD COLUMN pending_uploads_purged INTEGER NOT NULL DEFAULT 0
    CHECK (pending_uploads_purged >= 0);
ALTER TABLE maintenance_summaries ADD COLUMN files_purged INTEGER NOT NULL DEFAULT 0
    CHECK (files_purged >= 0);
