-- Trello imports: one row per import a member started with a Trello API key and token. Both are
-- kept only as ChaCha20-Poly1305 ciphertext under the app key while the import can still use
-- them; a finished, failed, cancelled or expired import has neither (enforced by a CHECK).
-- An import belongs to its creator: it goes with the user and with the workspace.
CREATE TABLE trello_imports (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN (
        'scanning', 'ready', 'queued', 'importing', 'completed', 'failed', 'cancelled', 'expired'
    )),
    key_ciphertext BLOB,
    token_ciphertext BLOB,
    trello_user_name TEXT,
    tree_json TEXT,
    selection_json TEXT,
    total INTEGER NOT NULL DEFAULT 0 CHECK (total >= 0),
    done INTEGER NOT NULL DEFAULT 0 CHECK (done >= 0),
    failed INTEGER NOT NULL DEFAULT 0 CHECK (failed >= 0),
    report_json TEXT,
    error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (
        (key_ciphertext IS NULL AND token_ciphertext IS NULL)
        OR status IN ('scanning', 'ready', 'queued', 'importing')
    )
);

CREATE INDEX trello_imports_user ON trello_imports (user_id, workspace_id, created_at);
CREATE INDEX trello_imports_workspace ON trello_imports (workspace_id);
CREATE INDEX trello_imports_status ON trello_imports (status, updated_at);

-- The import plan: the selected boards first, then every open card the job finds on their open
-- lists. `orbit_id` is the Orbit project or task id chosen before the row is created, so a
-- restarted import finds what it already made instead of creating it twice; it is a record, not
-- a reference (the row may be deleted later). `data_json` is the board mapping or the card as
-- listed (with its checklists and comments), kept until it is filled.
-- `state`: pending -> created (Orbit row exists) -> filled | failed.
CREATE TABLE trello_import_items (
    import_id TEXT NOT NULL REFERENCES trello_imports(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('board', 'card')),
    trello_id TEXT NOT NULL,
    board_id TEXT NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    title TEXT NOT NULL DEFAULT '',
    data_json TEXT,
    orbit_id TEXT,
    state TEXT NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'created', 'filled', 'failed')),
    error TEXT,
    PRIMARY KEY (import_id, kind, trello_id),
    UNIQUE (import_id, position)
);

CREATE INDEX trello_import_items_orbit ON trello_import_items (orbit_id);
