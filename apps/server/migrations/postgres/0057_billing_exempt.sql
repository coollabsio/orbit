-- Orbit Cloud: the root user's first workspace needs no subscription (apps/server/src/billing). At most one
-- workspace is exempt. Existing installations mark the oldest workspace the root user owns, deleted ones included.
ALTER TABLE workspaces ADD COLUMN billing_exempt BIGINT NOT NULL DEFAULT 0 CHECK (billing_exempt IN (0, 1));
CREATE UNIQUE INDEX workspaces_billing_exempt ON workspaces (billing_exempt) WHERE billing_exempt = 1;

UPDATE workspaces
SET billing_exempt = 1
WHERE id = (
    SELECT workspaces.id FROM workspaces
    JOIN memberships ON memberships.id = workspaces.owner_membership_id
    JOIN installation_state ON installation_state.id = 1 AND installation_state.root_user_id = memberships.user_id
    ORDER BY workspaces.created_at, workspaces.id
    LIMIT 1
);
