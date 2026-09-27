-- Docs favorites: each member's own ordered shortcut list of pages, per workspace. Only the
-- member sees it; the page stays in its space. Rows go with the page, the workspace, or the user.
CREATE TABLE page_favorites (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0 CHECK (position >= 0),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, page_id)
);

CREATE INDEX page_favorites_workspace_user_position
    ON page_favorites (workspace_id, user_id, position, page_id);
CREATE INDEX page_favorites_page ON page_favorites (page_id);

CREATE TRIGGER page_favorites_scope_insert
BEFORE INSERT ON page_favorites
WHEN NOT EXISTS (
    SELECT 1 FROM pages WHERE pages.id = NEW.page_id AND pages.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'favorite page must belong to the workspace');
END;

CREATE TRIGGER page_favorites_identity_immutable
BEFORE UPDATE OF workspace_id, user_id, page_id ON page_favorites
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.user_id <> OLD.user_id
    OR NEW.page_id <> OLD.page_id
BEGIN
    SELECT RAISE(ABORT, 'favorite identity is immutable');
END;
