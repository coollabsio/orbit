-- The default storage quota per workspace is 1 GB instead of 5 GB. Installations that kept the old default get the
-- new one; a quota the admin chose stays.
ALTER TABLE instance_settings ALTER COLUMN workspace_storage_quota_bytes SET DEFAULT 1000000000;
UPDATE instance_settings SET workspace_storage_quota_bytes = 1000000000 WHERE workspace_storage_quota_bytes = 5000000000;
