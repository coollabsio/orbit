-- Docs spaces: every page lives either in a shared teamspace (teamspace_id) or in one member's
-- private space (owner_id). A page's space is defined by its root; sub-pages share it.
CREATE TABLE teamspaces (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    icon TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX teamspaces_workspace_position ON teamspaces (workspace_id, position, id);

CREATE TRIGGER teamspaces_workspace_immutable
BEFORE UPDATE OF workspace_id ON teamspaces
WHEN NEW.workspace_id <> OLD.workspace_id
BEGIN
    SELECT RAISE(ABORT, 'teamspace workspace is immutable');
END;

ALTER TABLE pages ADD COLUMN teamspace_id TEXT REFERENCES teamspaces(id) ON DELETE CASCADE;
ALTER TABLE pages ADD COLUMN owner_id TEXT REFERENCES users(id) ON DELETE CASCADE;

-- Backfill: one "General" teamspace per workspace (trashed workspaces too, so restore works).
-- Ids are canonical lowercase UUIDv7: 48-bit millisecond timestamp, version 7, RFC 4122 variant.
INSERT INTO teamspaces (id, workspace_id, name, icon, position, version, created_by,
                        created_at, updated_at)
SELECT
    substr(seed.ts, 1, 8) || '-' || substr(seed.ts, 9, 4) || '-7' || substr(seed.r, 1, 3) || '-'
        || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(seed.r, 4, 3) || '-'
        || substr(seed.r, 7, 12),
    seed.workspace_id,
    'General',
    NULL,
    0,
    0,
    seed.created_by,
    seed.now_ms,
    seed.now_ms
FROM (
    SELECT
        workspaces.id AS workspace_id,
        (SELECT memberships.user_id FROM memberships
         WHERE memberships.id = workspaces.owner_membership_id) AS created_by,
        CAST(strftime('%s', 'now') AS INTEGER) * 1000
            + CAST(substr(strftime('%f', 'now'), 4, 3) AS INTEGER) AS now_ms,
        printf('%012x', CAST(strftime('%s', 'now') AS INTEGER) * 1000
            + CAST(substr(strftime('%f', 'now'), 4, 3) AS INTEGER)) AS ts,
        lower(hex(randomblob(9))) AS r
    FROM workspaces
) AS seed;

UPDATE pages SET teamspace_id = (
    SELECT teamspaces.id FROM teamspaces WHERE teamspaces.workspace_id = pages.workspace_id
);

CREATE INDEX pages_workspace_teamspace_parent_position
    ON pages (workspace_id, teamspace_id, parent_id, position, id);
CREATE INDEX pages_workspace_owner_parent_position
    ON pages (workspace_id, owner_id, parent_id, position, id);
CREATE INDEX pages_teamspace ON pages (teamspace_id);
CREATE INDEX pages_owner ON pages (owner_id);

-- The invariants below are created after the backfill so they never see half-migrated rows.
-- They fire on UPDATE OF the space columns only (not parent_id alone, except the parent check,
-- which skips NULL parents), so the ON DELETE SET NULL of pages.parent_id during a teamspace
-- cascade never trips them.
CREATE TRIGGER pages_space_exactly_one_insert
BEFORE INSERT ON pages
WHEN (NEW.teamspace_id IS NULL) = (NEW.owner_id IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'page must belong to exactly one space');
END;

CREATE TRIGGER pages_space_exactly_one_update
BEFORE UPDATE OF teamspace_id, owner_id ON pages
WHEN (NEW.teamspace_id IS NULL) = (NEW.owner_id IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'page must belong to exactly one space');
END;

CREATE TRIGGER pages_teamspace_scope_insert
BEFORE INSERT ON pages
WHEN NEW.teamspace_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM teamspaces
    WHERE teamspaces.id = NEW.teamspace_id AND teamspaces.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'page teamspace must belong to the workspace');
END;

CREATE TRIGGER pages_teamspace_scope_update
BEFORE UPDATE OF workspace_id, teamspace_id ON pages
WHEN NEW.teamspace_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM teamspaces
    WHERE teamspaces.id = NEW.teamspace_id AND teamspaces.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'page teamspace must belong to the workspace');
END;

CREATE TRIGGER pages_parent_space_insert
BEFORE INSERT ON pages
WHEN NEW.parent_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM pages AS parent
    WHERE parent.id = NEW.parent_id
      AND parent.teamspace_id IS NEW.teamspace_id
      AND parent.owner_id IS NEW.owner_id
)
BEGIN
    SELECT RAISE(ABORT, 'page must share its parent''s space');
END;

-- A subtree changing space is updated one depth level at a time (parents first).
CREATE TRIGGER pages_parent_space_update
BEFORE UPDATE OF parent_id, teamspace_id, owner_id ON pages
WHEN NEW.parent_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM pages AS parent
    WHERE parent.id = NEW.parent_id
      AND parent.teamspace_id IS NEW.teamspace_id
      AND parent.owner_id IS NEW.owner_id
)
BEGIN
    SELECT RAISE(ABORT, 'page must share its parent''s space');
END;
