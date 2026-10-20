-- Limits for a shared installation (Orbit Cloud) that the root user changes in Admin → General. 0 means unlimited.
-- max_owned_workspaces: how many workspaces (not in the trash) one account may own; instance admins are not limited.
-- workspace_storage_quota_bytes: how many bytes of files (attachment_blobs, counted once per workspace) one
-- workspace may store. 5 GB by default.
ALTER TABLE instance_settings ADD COLUMN max_owned_workspaces BIGINT NOT NULL DEFAULT 10
    CHECK (max_owned_workspaces BETWEEN 0 AND 1000000);
ALTER TABLE instance_settings ADD COLUMN workspace_storage_quota_bytes BIGINT NOT NULL DEFAULT 5000000000
    CHECK (workspace_storage_quota_bytes >= 0);
-- Counts the workspaces of one owner without a scan of every membership.
CREATE INDEX memberships_user_role ON memberships (user_id, role);
