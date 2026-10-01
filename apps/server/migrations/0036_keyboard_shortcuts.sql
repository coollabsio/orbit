-- Per-user keyboard shortcut overrides: { "<command id>": "<keys>" | null }, null = no shortcut.
-- A command that is not in the map keeps its default keys. The web app owns the command ids;
-- the server only bounds the size (apps/server/src/auth_routes.rs).
-- Shortcuts are personal, so the row belongs to the user, not to a workspace membership.
-- Users are never hard-deleted, so the user reference needs no ON DELETE action.
CREATE TABLE user_shortcuts (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    bindings_json TEXT NOT NULL CHECK (json_valid(bindings_json)),
    updated_at INTEGER NOT NULL
);
