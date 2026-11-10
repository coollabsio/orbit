-- Export modes (apps/server/src/workspace_export): `full` is the whole workspace as the requester
-- sees it (owners and admins), `personal` only the requester's private pages, private channels
-- and DMs (any member but a guest; for moving one's own content to another Orbit instance).
-- One export per (workspace, requester, mode) is queued or running at a time.
ALTER TABLE workspace_exports ADD COLUMN mode TEXT NOT NULL DEFAULT 'full'
    CHECK (mode IN ('full', 'personal'));

DROP INDEX workspace_exports_one_active;
CREATE UNIQUE INDEX workspace_exports_one_active ON workspace_exports (workspace_id, requested_by, mode)
    WHERE status IN ('queued', 'running');
