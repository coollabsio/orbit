-- Postgres: the same change as apps/server/migrations/20261124133003_custom_pages.sql
CREATE TABLE custom_pages (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    slug TEXT NOT NULL CHECK (length(slug) BETWEEN 1 AND 48),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 60),
    icon TEXT,
    url TEXT NOT NULL CHECK (length(url) BETWEEN 9 AND 2048),
    admins_only BIGINT NOT NULL DEFAULT 0 CHECK (admins_only IN (0, 1)),
    version BIGINT NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX custom_pages_workspace_slug ON custom_pages (workspace_id, slug);
