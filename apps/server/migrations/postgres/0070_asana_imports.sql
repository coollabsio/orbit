-- Asana imports: one row per import a member started with an Asana personal access token. The
-- token is kept only as ChaCha20-Poly1305 ciphertext under the app key while the import can
-- still use it; a finished, failed, cancelled or expired import has no token (enforced by a
-- CHECK). An import belongs to its creator: it goes with the user and with the workspace.
CREATE TABLE asana_imports (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN (
        'scanning', 'ready', 'queued', 'importing', 'completed', 'failed', 'cancelled', 'expired'
    )),
    token_ciphertext BYTEA,
    asana_user_name TEXT,
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

CREATE INDEX asana_imports_user ON asana_imports (user_id, workspace_id, created_at);
CREATE INDEX asana_imports_workspace ON asana_imports (workspace_id);
CREATE INDEX asana_imports_status ON asana_imports (status, updated_at);

-- The import plan: the selected projects first, then every task and subtask the job finds in
-- them, in creation order (`position`: parents before children). A task in several selected
-- projects is listed once, under the first. `orbit_id` is the Orbit project or task id chosen
-- before the row is created, so a restarted import finds what it already made instead of
-- creating it twice; it is a record, not a reference (the row may be deleted later).
-- `data_json` is the Asana task as listed, kept until the task is filled.
-- `state`: pending -> created (Orbit row exists) -> filled | failed.
CREATE TABLE asana_import_items (
    import_id TEXT NOT NULL REFERENCES asana_imports(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('project', 'task')),
    asana_gid TEXT NOT NULL,
    project_gid TEXT NOT NULL,
    parent_gid TEXT,
    position BIGINT NOT NULL CHECK (position >= 0),
    title TEXT NOT NULL DEFAULT '',
    data_json TEXT,
    orbit_id TEXT,
    state TEXT NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'created', 'filled', 'failed')),
    error TEXT,
    PRIMARY KEY (import_id, kind, asana_gid),
    UNIQUE (import_id, position)
);

CREATE INDEX asana_import_items_orbit ON asana_import_items (orbit_id);
