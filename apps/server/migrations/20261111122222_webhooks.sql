-- Outgoing webhooks (apps/server/src/webhooks): a workspace owner or admin subscribes a URL to task
-- events of chosen projects. Each matching event writes one delivery row, in the transaction of the
-- task change, and a `webhook.deliver` job sends it.
CREATE TABLE webhook_subscriptions (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    url TEXT NOT NULL CHECK (length(url) <= 2048),
    -- `whsec_…`, encrypted with the server's app key (secret_box).
    signing_secret BLOB NOT NULL,
    -- JSON array of event types, such as ["task.created","task.updated"].
    event_types TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX webhook_subscriptions_workspace ON webhook_subscriptions (workspace_id, enabled);

CREATE TABLE webhook_subscription_projects (
    subscription_id TEXT NOT NULL REFERENCES webhook_subscriptions(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    PRIMARY KEY (subscription_id, project_id)
);
CREATE INDEX webhook_subscription_projects_project ON webhook_subscription_projects (project_id);

-- `body` is the exact JSON sent on every attempt. `job_id` names the one job that may send it, so a
-- job left over from before a cancel or a manual retry does nothing.
CREATE TABLE webhook_deliveries (
    id TEXT PRIMARY KEY,
    subscription_id TEXT NOT NULL REFERENCES webhook_subscriptions(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    body TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'succeeded', 'failed', 'cancelled')),
    job_id TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_status INTEGER,
    last_error TEXT,
    last_attempt_at INTEGER,
    delivered_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX webhook_deliveries_subscription ON webhook_deliveries (subscription_id, created_at);
