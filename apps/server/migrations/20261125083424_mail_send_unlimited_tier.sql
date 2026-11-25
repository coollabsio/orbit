-- SQLite
-- The `unlimited` send tier: no send limits. It is the default of a self-hosted instance.
-- SQLite cannot alter a CHECK constraint, so mail_send_policies is rebuilt. No table refers to it.
CREATE TABLE mail_send_policies_backup AS SELECT * FROM mail_send_policies;
DROP TABLE mail_send_policies;

CREATE TABLE mail_send_policies (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    tier TEXT NOT NULL CHECK (tier IN ('new', 'standard', 'custom', 'unlimited')),
    custom_limits TEXT CHECK (custom_limits IS NULL OR json_valid(custom_limits)),
    -- Days a file link of an outgoing message stays valid.
    link_days INTEGER NOT NULL DEFAULT 30 CHECK (link_days BETWEEN 1 AND 365),
    first_release_at INTEGER,
    paused_at INTEGER,
    pause_reason TEXT,
    -- The first time sending was paused: a workspace that was ever paused is never promoted.
    first_paused_at INTEGER,
    resumed_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    -- The last resume: the rates that pause a workspace count only recipients released after it.
    resumed_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

INSERT INTO mail_send_policies (workspace_id, tier, custom_limits, link_days, first_release_at,
    paused_at, pause_reason, first_paused_at, resumed_by, resumed_at, created_at, updated_at)
SELECT workspace_id, tier, custom_limits, link_days, first_release_at, paused_at, pause_reason,
    first_paused_at, resumed_by, resumed_at, created_at, updated_at
FROM mail_send_policies_backup;
DROP TABLE mail_send_policies_backup;
