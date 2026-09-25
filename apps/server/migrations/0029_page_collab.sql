-- Docs real-time co-editing: each page's collaborative (Yjs) document. A page gets its row the
-- first time it is opened for co-editing (or its content is replaced while live): the server
-- converts `pages.content_json` into a Yjs document then. Edits append to
-- `page_collab_updates` in small batches; compaction folds the log into `snapshot` and deletes
-- the folded rows. `pages.content_json` stays the JSON projection of the document (search,
-- history, API, duplicate). `epoch` changes whenever connected clients must drop their local
-- copy (backup restore, converter change); pages without a row report `page_collab_meta`'s
-- generation, which a backup restore rotates too. Rows go with their page and workspace.
CREATE TABLE page_collab_docs (
    page_id TEXT PRIMARY KEY REFERENCES pages(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    -- Yjs v1 update encoding the whole document as of `snapshot_seq`.
    snapshot BLOB NOT NULL,
    -- The last `page_collab_updates.seq` folded into `snapshot` (0 when none).
    snapshot_seq INTEGER NOT NULL DEFAULT 0 CHECK (snapshot_seq >= 0),
    epoch TEXT NOT NULL CHECK (length(epoch) BETWEEN 1 AND 64),
    -- Version of the JSON <-> Yjs converter (and editor schema) that built the document.
    converter_version INTEGER NOT NULL,
    -- The last update seq whose state is written to `pages.content_json`.
    projected_seq INTEGER NOT NULL DEFAULT 0 CHECK (projected_seq >= 0),
    -- When a co-editing projection last recorded a `page.updated` audit event (throttle).
    audited_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX page_collab_docs_workspace ON page_collab_docs (workspace_id);

CREATE TABLE page_collab_updates (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    -- Yjs v1 update (one batch of merged client updates).
    data BLOB NOT NULL,
    -- The last editor in the batch; NULL for server-authored changes or deleted users.
    user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX page_collab_updates_page ON page_collab_updates (page_id, seq);
CREATE INDEX page_collab_updates_workspace ON page_collab_updates (workspace_id);
CREATE INDEX page_collab_updates_user ON page_collab_updates (user_id);

CREATE TABLE page_collab_meta (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    generation TEXT NOT NULL CHECK (length(generation) BETWEEN 1 AND 64)
);

INSERT INTO page_collab_meta (id, generation) VALUES (1, lower(hex(randomblob(16))));

CREATE TRIGGER page_collab_docs_scope_insert
BEFORE INSERT ON page_collab_docs
WHEN NOT EXISTS (
    SELECT 1 FROM pages WHERE pages.id = NEW.page_id AND pages.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'collaborative document must belong to the page workspace');
END;

CREATE TRIGGER page_collab_docs_scope_immutable
BEFORE UPDATE OF workspace_id, page_id ON page_collab_docs
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.page_id <> OLD.page_id
BEGIN
    SELECT RAISE(ABORT, 'collaborative document scope is immutable');
END;

CREATE TRIGGER page_collab_updates_scope_insert
BEFORE INSERT ON page_collab_updates
WHEN NOT EXISTS (
    SELECT 1 FROM pages WHERE pages.id = NEW.page_id AND pages.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'collaborative update must belong to the page workspace');
END;

CREATE TRIGGER page_collab_updates_scope_immutable
BEFORE UPDATE OF workspace_id, page_id ON page_collab_updates
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.page_id <> OLD.page_id
BEGIN
    SELECT RAISE(ABORT, 'collaborative update scope is immutable');
END;
