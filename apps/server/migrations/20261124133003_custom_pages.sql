-- SQLite
-- Custom pages (#94): web pages that a workspace shows in its sidebar, framed in a sandboxed iframe. The URL's origin
-- must be in ORBIT_FRAME_ORIGINS; the server checks it, as the list can change without a migration.
CREATE TABLE custom_pages (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    slug TEXT NOT NULL CHECK (length(slug) BETWEEN 1 AND 48),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 60),
    icon TEXT,
    url TEXT NOT NULL CHECK (length(url) BETWEEN 9 AND 2048),
    admins_only INTEGER NOT NULL DEFAULT 0 CHECK (admins_only IN (0, 1)),
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX custom_pages_workspace_slug ON custom_pages (workspace_id, slug);
