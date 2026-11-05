-- Linear imports: one row per import a member started with a Linear personal API key. The key
-- is kept only as ChaCha20-Poly1305 ciphertext under the app key while the import can still
-- use it; a finished, failed, cancelled or expired import has no key (enforced by a CHECK).
-- An import belongs to its creator: it goes with the user and with the workspace.
CREATE TABLE linear_imports (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN (
        'scanning', 'ready', 'queued', 'importing', 'completed', 'failed', 'cancelled', 'expired'
    )),
    token_ciphertext BYTEA,
    linear_user_name TEXT,
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

CREATE INDEX linear_imports_user ON linear_imports (user_id, workspace_id, created_at);
CREATE INDEX linear_imports_workspace ON linear_imports (workspace_id);
CREATE INDEX linear_imports_status ON linear_imports (status, updated_at);

-- The import plan: the selected teams first, then every issue the job finds in them (parents
-- before sub-issues), then the relations between issues. `orbit_id` is the Orbit project or
-- task id chosen before the row is created, so a restarted import finds what it already made
-- instead of creating it twice; it is a record, not a reference (the row may be deleted later).
-- `data_json` is the Linear team mapping, issue or relation as listed, kept until it is filled.
-- `state`: pending -> created (Orbit row exists) -> filled | failed.
CREATE TABLE linear_import_items (
    import_id TEXT NOT NULL REFERENCES linear_imports(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('team', 'issue', 'relation')),
    linear_id TEXT NOT NULL,
    team_id TEXT NOT NULL,
    parent_id TEXT,
    position BIGINT NOT NULL CHECK (position >= 0),
    title TEXT NOT NULL DEFAULT '',
    data_json TEXT,
    orbit_id TEXT,
    state TEXT NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'created', 'filled', 'failed')),
    error TEXT,
    PRIMARY KEY (import_id, kind, linear_id),
    UNIQUE (import_id, position)
);

CREATE INDEX linear_import_items_orbit ON linear_import_items (orbit_id);
