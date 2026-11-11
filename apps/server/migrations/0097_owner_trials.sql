-- Orbit Cloud: the trial belongs to the owner account, not to the workspace, so deleting a workspace and creating a
-- new one does not start a new trial. users.trial_started_at is set when the account creates its first workspace
-- that is not billing-exempt, and is never cleared; each new workspace copies it to workspaces.trial_started_at,
-- which the trial reads (apps/server/src/billing/mod.rs, access).
ALTER TABLE users ADD COLUMN trial_started_at BIGINT;
ALTER TABLE workspaces ADD COLUMN trial_started_at BIGINT NOT NULL DEFAULT 0;
-- Existing trials keep their end.
UPDATE workspaces SET trial_started_at = created_at;
UPDATE users SET trial_started_at = (
    SELECT MIN(workspaces.created_at) FROM memberships
    JOIN workspaces ON workspaces.id = memberships.workspace_id
    WHERE memberships.user_id = users.id AND memberships.role = 'owner' AND workspaces.billing_exempt = 0
);
