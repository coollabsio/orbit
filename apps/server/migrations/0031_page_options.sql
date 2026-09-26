-- Docs page options: per-page full width and lock, and each member's recently visited pages.
-- `full_width` is a layout preference of the page (everyone sees it). A locked page
-- (`locked_at` set) refuses title, icon, cover and content writes until it is unlocked;
-- `locked_by` names who locked it (NULL once that user is gone).
ALTER TABLE pages ADD COLUMN full_width INTEGER NOT NULL DEFAULT 0 CHECK (full_width IN (0, 1));
ALTER TABLE pages ADD COLUMN locked_at INTEGER;
ALTER TABLE pages ADD COLUMN locked_by TEXT REFERENCES users(id) ON DELETE SET NULL
    CHECK (locked_by IS NULL OR locked_at IS NOT NULL);

CREATE INDEX pages_locked_by ON pages (locked_by) WHERE locked_by IS NOT NULL;

-- When each member last opened a page, per workspace (the "Recent" list). Only that member
-- sees their rows; only live pages they can still see are listed. Rows go with the page, the
-- workspace, or the user.
CREATE TABLE page_visits (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    visited_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, page_id)
);

CREATE INDEX page_visits_workspace_user_recent
    ON page_visits (workspace_id, user_id, visited_at DESC, page_id);
CREATE INDEX page_visits_page ON page_visits (page_id);

CREATE TRIGGER page_visits_scope_insert
BEFORE INSERT ON page_visits
WHEN NOT EXISTS (
    SELECT 1 FROM pages WHERE pages.id = NEW.page_id AND pages.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'visited page must belong to the workspace');
END;

CREATE TRIGGER page_visits_identity_immutable
BEFORE UPDATE OF workspace_id, user_id, page_id ON page_visits
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.user_id <> OLD.user_id
    OR NEW.page_id <> OLD.page_id
BEGIN
    SELECT RAISE(ABORT, 'page visit identity is immutable');
END;
