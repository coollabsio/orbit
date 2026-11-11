-- Orbit imports: one row per import of another Orbit workspace's export archive, pulled from the
-- other instance with an export API token (`source_kind` 'url') or uploaded as a ZIP ('upload').
-- The token is kept only as ChaCha20-Poly1305 ciphertext under the app key while the import can
-- still use it; a final state has no token (enforced by a CHECK). The archive is stored like an
-- Obsidian upload (`archive_location`, key `imports/{workspace_id}/{id}.zip`) until a final state.
-- `mode` is the archive's mode: 'full' (owners and admins of this workspace) or 'personal' (any
-- member but a guest; only private pages, private channels and DMs land in the importer's
-- account). An import belongs to its creator: it goes with the user and with the workspace.
CREATE TABLE orbit_imports (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN (
        'scanning', 'ready', 'queued', 'importing', 'completed', 'failed', 'cancelled', 'expired'
    )),
    -- While scanning: where the fetch is.
    phase TEXT CHECK (phase IN ('exporting', 'downloading', 'reading')),
    mode TEXT CHECK (mode IN ('full', 'personal')),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('url', 'upload')),
    source_url TEXT,
    -- From manifest.json, once read; the dedupe key of `orbit_import_links`.
    source_workspace_id TEXT,
    source_workspace_name TEXT,
    exported_by_email TEXT,
    token_ciphertext BYTEA,
    remote_export_id TEXT,
    archive_location TEXT CHECK (archive_location IN ('local', 'bucket', 'workspace_bucket')),
    archive_bytes BIGINT NOT NULL DEFAULT 0 CHECK (archive_bytes >= 0),
    tree_json TEXT,
    selection_json TEXT,
    total BIGINT NOT NULL DEFAULT 0 CHECK (total >= 0),
    done BIGINT NOT NULL DEFAULT 0 CHECK (done >= 0),
    failed BIGINT NOT NULL DEFAULT 0 CHECK (failed >= 0),
    report_json TEXT,
    error TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CHECK (token_ciphertext IS NULL OR status IN ('scanning', 'ready', 'queued', 'importing'))
);

CREATE INDEX orbit_imports_user ON orbit_imports (user_id, workspace_id, created_at);
CREATE INDEX orbit_imports_workspace ON orbit_imports (workspace_id);
CREATE INDEX orbit_imports_status ON orbit_imports (status, updated_at);

-- The import plan, written by the import job from the archive and the selection: teamspaces,
-- projects, tasks (parents before sub-tasks), pages (parents first) and channels, in `position`
-- order. `source_id` is the record's id in the source workspace; `path` the archive entry that
-- holds it. `orbit_id` is the Orbit id chosen before the row is created, so a restarted import
-- finds what it already made; it is a record, not a reference.
-- `state`: pending -> created (Orbit row exists) -> filled | failed.
CREATE TABLE orbit_import_items (
    import_id TEXT NOT NULL REFERENCES orbit_imports(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('teamspace', 'project', 'task', 'page', 'channel')),
    source_id TEXT NOT NULL,
    parent_id TEXT,
    path TEXT NOT NULL DEFAULT '',
    position BIGINT NOT NULL CHECK (position >= 0),
    title TEXT NOT NULL DEFAULT '',
    orbit_id TEXT,
    state TEXT NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'created', 'filled', 'failed')),
    error TEXT,
    PRIMARY KEY (import_id, kind, source_id),
    UNIQUE (import_id, position)
);

-- What every Orbit import into this workspace created for a record of a source workspace, so a
-- second import of the same source (a URL fetch or a ZIP upload, in any order) reuses the row
-- instead of making a second one. `scope` is '' for full imports and the importer's user id for
-- personal imports: the source ids come from the archive, which a personal importer controls,
-- so a personal import may only reuse its own rows, and reads scope '' only to leave alone what
-- a full import created. `orbit_id` is a record, not a reference: the row may be deleted later,
-- and the importer then creates it again.
CREATE TABLE orbit_import_links (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    source_workspace_id TEXT NOT NULL,
    scope TEXT NOT NULL,
    kind TEXT NOT NULL,
    source_id TEXT NOT NULL,
    orbit_id TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (workspace_id, source_workspace_id, scope, kind, source_id)
);
