-- Orbit Cloud billing (ORBIT_CLOUD=true): the Stripe customer and subscription of each workspace.
-- The row is written when the owner first opens checkout (customer only) and kept up to date by the
-- billing.stripe_sync job, which re-reads the subscription from Stripe after each webhook.
-- status is the Stripe subscription status; no row or a status other than active, trialing or
-- past_due makes the workspace read-only (apps/server/src/billing). Times are unix milliseconds.
CREATE TABLE workspace_subscriptions (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    stripe_customer_id TEXT NOT NULL UNIQUE,
    stripe_subscription_id TEXT UNIQUE,
    plan TEXT,
    status TEXT,
    cancel_at_period_end INTEGER NOT NULL DEFAULT 0 CHECK (cancel_at_period_end IN (0, 1)),
    current_period_end INTEGER,
    updated_at INTEGER NOT NULL
);

-- Stripe event ids already accepted by the webhook, so a redelivered event queues no second job.
-- Stripe retries for three days; the webhook deletes rows older than 30 days.
CREATE TABLE stripe_events (
    event_id TEXT PRIMARY KEY,
    received_at INTEGER NOT NULL
);

CREATE INDEX stripe_events_received_at ON stripe_events (received_at);
